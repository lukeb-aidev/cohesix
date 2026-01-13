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
use alloc::{string::String, vec::Vec};
use core::fmt::{self, Write};
use core::str;
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
const POLICY_CTL_PATH: &str = "/policy/ctl";
const POLICY_RULES_PATH: &str = "/policy/rules";
const POLICY_ROOT_PATH: &str = "/policy";
const ACTIONS_QUEUE_PATH: &str = "/actions/queue";
const ACTIONS_ROOT_PATH: &str = "/actions";
const MAX_POLICY_PATH_COMPONENTS: usize = 8;
const MAX_ACTION_ID_LEN: usize = 64;
const SYSTEMD_UNITS: [&str; 2] = ["cohesix-agent.service", "ssh.service"];
const K8S_NODES: [&str; 1] = ["node-1"];
const NVIDIA_GPUS: [&str; 1] = ["0"];

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
    session_role: Option<SessionRoleLabel>,
    session_ticket: Option<String>,
    next_worker_id: u32,
    telemetry: generated::TelemetryConfig,
    workers: HeaplessVec<WorkerTelemetry, MAX_WORKERS>,
    host: HostState,
    policy: PolicyState,
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
    /// Operation was denied by policy or capability checks.
    Permission,
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
            Self::Permission => write!(f, "EPERM"),
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
            session_role: None,
            session_ticket: None,
            next_worker_id: 1,
            telemetry: generated::telemetry_config(),
            workers: HeaplessVec::new(),
            host: HostState::new(),
            policy: PolicyState::new(),
        }
    }

    /// Reset per-session state after a console disconnect.
    pub fn reset_session(&mut self) {
        self.attached = false;
        self.session_role = None;
        self.session_ticket = None;
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
            self.update_session_context(role, ticket);
            return Ok(());
        }
        #[cfg(feature = "kernel")]
        {
            boot_log::notify_bridge_attached();
            if boot_log::bridge_disabled() || boot_log::ep_only_active() {
                self.attached = true;
                boot_tracer().advance(BootPhase::EPAttachOk);
                self.update_session_context(role, ticket);
                return Ok(());
            }
            return Err(NineDoorBridgeError::AttachTimeout);
        }
        #[cfg(not(feature = "kernel"))]
        {
            self.update_session_context(role, ticket);
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
        if self.policy.enabled {
            if path == POLICY_CTL_PATH {
                self.policy.append_policy_ctl(payload)?;
                return Ok(());
            }
            if path == ACTIONS_QUEUE_PATH {
                let role = self.role_label();
                let ticket = String::from(self.ticket_label());
                self.policy
                    .append_action_queue(payload, role, ticket.as_str())?;
                return Ok(());
            }
        }
        if path == QUEEN_CTL_PATH {
            self.apply_policy_gate(path)?;
            return self.handle_queen_ctl(payload);
        }
        if let Some(control) = self.host.control_label(path) {
            if !self.is_queen() {
                self.log_host_write(path, Some(control), HostWriteOutcome::Denied, None);
                return Err(NineDoorBridgeError::Permission);
            }
            self.apply_policy_gate(path)?;
            self.host.update_value(path, payload);
            self.log_host_write(
                path,
                Some(control),
                HostWriteOutcome::Allowed,
                Some(payload.len()),
            );
            return Ok(());
        }
        if self.host.entry_value(path).is_some() {
            self.log_host_write(path, None, HostWriteOutcome::Denied, None);
            return Err(NineDoorBridgeError::Permission);
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
        if self.policy.enabled {
            if path == POLICY_RULES_PATH {
                return script_lines(self.policy.rules_json());
            }
            if path == POLICY_CTL_PATH {
                return lines_from_bytes(self.policy.ctl_log());
            }
            if path == ACTIONS_QUEUE_PATH {
                return lines_from_bytes(self.policy.queue_log());
            }
            if let Some(action_id) = parse_action_status_path(path) {
                return self.action_status_lines(action_id);
            }
        }
        if let Some(value) = self.host.entry_value(path) {
            return lines_from_text(value);
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
        if path == "/" {
            let mut output = HeaplessVec::new();
            for entry in ["gpu", "kmesg", "log", "proc", "queen", "trace", "worker"] {
                push_list_entry(&mut output, entry)?;
            }
            if self.host.enabled {
                push_list_entry(&mut output, self.host.mount_label())?;
            }
            if self.policy.enabled {
                push_list_entry(&mut output, "policy")?;
                push_list_entry(&mut output, "actions")?;
            }
            return Ok(output);
        }
        if path == "/log" {
            return list_from_slice(&["queen.log"]);
        }
        if path == "/proc" {
            return list_from_slice(&["boot", "tests"]);
        }
        if path == "/proc/tests" {
            return list_from_slice(&[
                "selftest_quick.coh",
                "selftest_full.coh",
                "selftest_negative.coh",
            ]);
        }
        if path == "/queen" {
            return list_from_slice(&["ctl"]);
        }
        if path == "/trace" {
            return list_from_slice(&["ctl", "events"]);
        }
        if path == "/worker" || path == "/gpu" {
            return Ok(HeaplessVec::new());
        }
        if self.policy.enabled {
            if path == POLICY_ROOT_PATH {
                return list_from_slice(&["ctl", "rules"]);
            }
            if path == ACTIONS_ROOT_PATH {
                return list_from_slice(&["queue"]);
            }
        }
        if let Some(output) = self.host.list(path) {
            return Ok(output);
        }
        Err(NineDoorBridgeError::InvalidPath)
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

    fn update_session_context(&mut self, role: &str, ticket: Option<&str>) {
        self.session_role = parse_session_role(role);
        self.session_ticket = ticket.map(String::from);
    }

    fn role_label(&self) -> &'static str {
        match self.session_role {
            Some(SessionRoleLabel::Queen) => "queen",
            Some(SessionRoleLabel::WorkerHeartbeat) => "worker-heartbeat",
            Some(SessionRoleLabel::WorkerGpu) => "worker-gpu",
            None => "unauthenticated",
        }
    }

    fn ticket_label(&self) -> &str {
        self.session_ticket.as_deref().unwrap_or("none")
    }

    fn is_queen(&self) -> bool {
        matches!(self.session_role, Some(SessionRoleLabel::Queen))
    }

    fn apply_policy_gate(&mut self, path: &str) -> Result<(), NineDoorBridgeError> {
        let decision = self.policy.consume_gate(path);
        match decision {
            PolicyGateDecision::Allowed(allowance) => {
                if matches!(allowance, PolicyGateAllowance::Action { .. }) {
                    self.log_policy_gate_allow(path, &allowance);
                }
                Ok(())
            }
            PolicyGateDecision::Denied(denial) => {
                self.log_policy_gate_deny(path, &denial);
                Err(NineDoorBridgeError::Permission)
            }
        }
    }

    fn action_status_lines(
        &self,
        action_id: &str,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let action = self
            .policy
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let decision = action.decision.as_str();
        let state = if action.consumed { "consumed" } else { "queued" };
        let max_len = core::cmp::min(
            self.policy.limits.status_max_bytes as usize,
            DEFAULT_LINE_CAPACITY,
        );
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let wrote = write!(
            line,
            "{{\"id\":\"{}\",\"decision\":\"{}\",\"state\":\"{}\"}}",
            action.id,
            decision,
            state
        )
        .is_ok();
        if !wrote || line.len() > max_len {
            line.clear();
            let _ = write!(line, "{{\"id\":\"{}\",\"state\":\"oversize\"}}", action.id);
        }
        let mut output = HeaplessVec::new();
        push_boot_line(&mut output, line.as_str())?;
        Ok(output)
    }

    fn log_policy_gate_allow(&self, path: &str, allowance: &PolicyGateAllowance) {
        let PolicyGateAllowance::Action { id, target } = allowance else {
            return;
        };
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "policy-gate outcome=allow role={} ticket={} id={} target={} path={}",
            self.role_label(),
            self.ticket_label(),
            id,
            target,
            path
        );
        log_buffer::append_log_line(line.as_str());
    }

    fn log_policy_gate_deny(&self, path: &str, denial: &PolicyGateDenial) {
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        match denial {
            PolicyGateDenial::Missing => {
                let _ = write!(
                    line,
                    "policy-gate outcome=deny role={} ticket={} reason=missing-approval path={}",
                    self.role_label(),
                    self.ticket_label(),
                    path
                );
            }
            PolicyGateDenial::Action { id, target } => {
                let _ = write!(
                    line,
                    "policy-gate outcome=deny role={} ticket={} id={} target={} path={}",
                    self.role_label(),
                    self.ticket_label(),
                    id,
                    target,
                    path
                );
            }
        }
        log_buffer::append_log_line(line.as_str());
    }

    fn log_host_write(
        &self,
        path: &str,
        control: Option<&'static str>,
        outcome: HostWriteOutcome,
        bytes: Option<usize>,
    ) {
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "host-write outcome={} role={} ticket={} path={}",
            outcome.as_str(),
            self.role_label(),
            self.ticket_label(),
            path
        );
        if let Some(control) = control {
            let _ = write!(line, " control={control}");
        }
        if let Some(bytes) = bytes {
            let _ = write!(line, " bytes={bytes}");
        }
        log_buffer::append_log_line(line.as_str());
    }
}

#[derive(Debug, Clone, Copy)]
enum SessionRoleLabel {
    Queen,
    WorkerHeartbeat,
    WorkerGpu,
}

fn parse_session_role(role: &str) -> Option<SessionRoleLabel> {
    if role.eq_ignore_ascii_case("queen") {
        Some(SessionRoleLabel::Queen)
    } else if role.eq_ignore_ascii_case("worker")
        || role.eq_ignore_ascii_case("worker-heartbeat")
    {
        Some(SessionRoleLabel::WorkerHeartbeat)
    } else if role.eq_ignore_ascii_case("worker-gpu") {
        Some(SessionRoleLabel::WorkerGpu)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
enum HostWriteOutcome {
    Allowed,
    Denied,
}

impl HostWriteOutcome {
    fn as_str(self) -> &'static str {
        match self {
            HostWriteOutcome::Allowed => "allow",
            HostWriteOutcome::Denied => "deny",
        }
    }
}

#[derive(Debug)]
struct HostEntry {
    path: String,
    value: String,
    control: Option<&'static str>,
}

#[derive(Debug)]
struct HostState {
    enabled: bool,
    mount_at: String,
    mount_parts: Vec<String>,
    providers: &'static [generated::HostProvider],
    entries: Vec<HostEntry>,
}

impl HostState {
    fn new() -> Self {
        let config = generated::host_config();
        let mount_trimmed = config.mount_at.trim_end_matches('/');
        let mount_at = if mount_trimmed.is_empty() {
            config.mount_at
        } else {
            mount_trimmed
        };
        let mount_at = String::from(mount_at);
        let mount_parts = mount_at
            .split('/')
            .filter(|seg| !seg.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        let mut state = Self {
            enabled: config.enable,
            mount_at,
            mount_parts,
            providers: config.providers,
            entries: Vec::new(),
        };
        if state.enabled {
            state.build_entries();
        }
        state
    }

    fn mount_label(&self) -> &str {
        self.mount_parts
            .first()
            .map(String::as_str)
            .unwrap_or("host")
    }

    fn list(
        &self,
        path: &str,
    ) -> Option<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>> {
        if !self.enabled {
            return None;
        }
        let parts = split_path_segments(path);
        if parts.is_empty() {
            return None;
        }
        if parts.len() < self.mount_parts.len() {
            if self.mount_parts_prefix(&parts) {
                let next = &self.mount_parts[parts.len()];
                return list_from_slice(&[next.as_str()]).ok();
            }
            return None;
        }
        if parts.len() == self.mount_parts.len() {
            if !self.mount_parts_match(&parts) {
                return None;
            }
            let mut output = HeaplessVec::new();
            for provider in self.providers.iter().copied() {
                let label = host_provider_label(provider);
                if push_list_entry(&mut output, label).is_err() {
                    return None;
                }
            }
            return Some(output);
        }
        if !self.mount_parts_match(&parts) {
            return None;
        }
        let rel = &parts[self.mount_parts.len()..];
        match rel {
            ["systemd"] if self.has_provider(generated::HostProvider::Systemd) => {
                list_from_slice(&SYSTEMD_UNITS).ok()
            }
            ["systemd", unit]
                if self.has_provider(generated::HostProvider::Systemd)
                    && SYSTEMD_UNITS.iter().any(|entry| entry == unit) =>
            {
                list_from_slice(&["status", "restart"]).ok()
            }
            ["k8s"] if self.has_provider(generated::HostProvider::K8s) => {
                list_from_slice(&["node"]).ok()
            }
            ["k8s", "node"] if self.has_provider(generated::HostProvider::K8s) => {
                list_from_slice(&K8S_NODES).ok()
            }
            ["k8s", "node", node]
                if self.has_provider(generated::HostProvider::K8s)
                    && K8S_NODES.iter().any(|entry| entry == node) =>
            {
                list_from_slice(&["cordon", "drain"]).ok()
            }
            ["nvidia"] if self.has_provider(generated::HostProvider::Nvidia) => {
                list_from_slice(&["gpu"]).ok()
            }
            ["nvidia", "gpu"] if self.has_provider(generated::HostProvider::Nvidia) => {
                list_from_slice(&NVIDIA_GPUS).ok()
            }
            ["nvidia", "gpu", gpu]
                if self.has_provider(generated::HostProvider::Nvidia)
                    && NVIDIA_GPUS.iter().any(|entry| entry == gpu) =>
            {
                list_from_slice(&["status", "power_cap", "thermal"]).ok()
            }
            ["jetson"] if self.has_provider(generated::HostProvider::Jetson) => {
                Some(HeaplessVec::new())
            }
            ["net"] if self.has_provider(generated::HostProvider::Net) => Some(HeaplessVec::new()),
            _ => None,
        }
    }

    fn entry_value(&self, path: &str) -> Option<&str> {
        if !self.enabled {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.value.as_str())
    }

    fn control_label(&self, path: &str) -> Option<&'static str> {
        if !self.enabled {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.control)
    }

    fn update_value(&mut self, path: &str, value: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            entry.value = String::from(value);
            return true;
        }
        false
    }

    fn has_provider(&self, provider: generated::HostProvider) -> bool {
        self.providers.iter().any(|entry| *entry == provider)
    }

    fn mount_parts_prefix(
        &self,
        parts: &HeaplessVec<&str, MAX_POLICY_PATH_COMPONENTS>,
    ) -> bool {
        for (idx, part) in parts.iter().enumerate() {
            if self.mount_parts.get(idx).map(String::as_str) != Some(*part) {
                return false;
            }
        }
        true
    }

    fn mount_parts_match(
        &self,
        parts: &HeaplessVec<&str, MAX_POLICY_PATH_COMPONENTS>,
    ) -> bool {
        if parts.len() < self.mount_parts.len() {
            return false;
        }
        for (part, mount) in parts.iter().zip(self.mount_parts.iter()) {
            if *part != mount.as_str() {
                return false;
            }
        }
        true
    }

    fn build_entries(&mut self) {
        for provider in self.providers.iter().copied() {
            match provider {
                generated::HostProvider::Systemd => {
                    for unit in SYSTEMD_UNITS {
                        self.push_entry(&["systemd", unit, "status"], "active", None);
                        self.push_entry(
                            &["systemd", unit, "restart"],
                            "",
                            Some("systemd.restart"),
                        );
                    }
                }
                generated::HostProvider::K8s => {
                    for node in K8S_NODES {
                        self.push_entry(
                            &["k8s", "node", node, "cordon"],
                            "",
                            Some("k8s.cordon"),
                        );
                        self.push_entry(
                            &["k8s", "node", node, "drain"],
                            "",
                            Some("k8s.drain"),
                        );
                    }
                }
                generated::HostProvider::Nvidia => {
                    for gpu in NVIDIA_GPUS {
                        self.push_entry(&["nvidia", "gpu", gpu, "status"], "ok", None);
                        self.push_entry(
                            &["nvidia", "gpu", gpu, "power_cap"],
                            "",
                            Some("nvidia.power_cap"),
                        );
                        self.push_entry(&["nvidia", "gpu", gpu, "thermal"], "42C", None);
                    }
                }
                generated::HostProvider::Jetson | generated::HostProvider::Net => {}
            }
        }
    }

    fn push_entry(&mut self, parts: &[&str], value: &str, control: Option<&'static str>) {
        let path = join_path(self.mount_at.as_str(), parts);
        self.entries.push(HostEntry {
            path,
            value: String::from(value),
            control,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicyDecision {
    Approve,
    Deny,
}

impl PolicyDecision {
    fn as_str(self) -> &'static str {
        match self {
            PolicyDecision::Approve => "approve",
            PolicyDecision::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone)]
struct PolicyAction {
    id: String,
    target: String,
    decision: PolicyDecision,
    consumed: bool,
}

#[derive(Debug)]
struct PolicyState {
    enabled: bool,
    limits: generated::PolicyLimits,
    rules: &'static [generated::PolicyRule],
    rules_json: &'static str,
    ctl_log: Vec<u8>,
    queue_log: Vec<u8>,
    actions: Vec<PolicyAction>,
}

impl PolicyState {
    fn new() -> Self {
        let config = generated::policy_config();
        Self {
            enabled: config.enable,
            limits: config.limits,
            rules: config.rules,
            rules_json: generated::policy_rules_json(),
            ctl_log: Vec::new(),
            queue_log: Vec::new(),
            actions: Vec::new(),
        }
    }

    fn rules_json(&self) -> &str {
        self.rules_json
    }

    fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    fn queue_log(&self) -> &[u8] {
        &self.queue_log
    }

    fn append_policy_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        validate_json_envelope(payload)?;
        append_log_bytes(&mut self.ctl_log, payload, self.limits.ctl_max_bytes)
    }

    fn append_action_queue(
        &mut self,
        payload: &str,
        role: &str,
        ticket: &str,
    ) -> Result<(), NineDoorBridgeError> {
        let actions = parse_action_lines(payload)?;
        if actions.is_empty() {
            return Ok(());
        }
        let max_entries = self.limits.queue_max_entries as usize;
        if self.actions.len() + actions.len() > max_entries {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        for action in actions.iter() {
            if self.actions.iter().any(|entry| entry.id == action.id) {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
        }
        append_log_bytes(&mut self.queue_log, payload, self.limits.queue_max_bytes)?;
        for action in actions {
            log_policy_action(role, ticket, &action);
            self.actions.push(action);
        }
        Ok(())
    }

    fn consume_gate(&mut self, path: &str) -> PolicyGateDecision {
        if !self.enabled {
            return PolicyGateDecision::Allowed(PolicyGateAllowance::Ungated);
        }
        let normalized = normalize_path(path);
        if !self
            .rules
            .iter()
            .any(|rule| path_matches_pattern(rule.target, normalized.as_str()))
        {
            return PolicyGateDecision::Allowed(PolicyGateAllowance::NotRequired);
        }
        if let Some(action) = self
            .actions
            .iter_mut()
            .find(|action| !action.consumed && action.target == normalized)
        {
            action.consumed = true;
            return match action.decision {
                PolicyDecision::Approve => PolicyGateDecision::Allowed(PolicyGateAllowance::Action {
                    id: action.id.clone(),
                    target: action.target.clone(),
                }),
                PolicyDecision::Deny => PolicyGateDecision::Denied(PolicyGateDenial::Action {
                    id: action.id.clone(),
                    target: action.target.clone(),
                }),
            };
        }
        PolicyGateDecision::Denied(PolicyGateDenial::Missing)
    }
}

#[derive(Debug)]
enum PolicyGateDecision {
    Allowed(PolicyGateAllowance),
    Denied(PolicyGateDenial),
}

#[derive(Debug)]
enum PolicyGateAllowance {
    Ungated,
    NotRequired,
    Action { id: String, target: String },
}

#[derive(Debug)]
enum PolicyGateDenial {
    Missing,
    Action { id: String, target: String },
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

fn host_provider_label(provider: generated::HostProvider) -> &'static str {
    match provider {
        generated::HostProvider::Systemd => "systemd",
        generated::HostProvider::K8s => "k8s",
        generated::HostProvider::Nvidia => "nvidia",
        generated::HostProvider::Jetson => "jetson",
        generated::HostProvider::Net => "net",
    }
}

fn join_path(mount: &str, parts: &[&str]) -> String {
    let mut out = String::new();
    out.push_str(mount);
    for part in parts {
        if !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(part);
    }
    out
}

fn normalize_path(path: &str) -> String {
    let segments = split_path_segments(path);
    if segments.is_empty() {
        return String::from(path);
    }
    let mut out = String::new();
    out.push('/');
    for (idx, segment) in segments.iter().enumerate() {
        if idx > 0 {
            out.push('/');
        }
        out.push_str(segment);
    }
    out
}

fn split_path_segments(path: &str) -> HeaplessVec<&str, MAX_POLICY_PATH_COMPONENTS> {
    let mut segments = HeaplessVec::new();
    for segment in path.split('/').filter(|seg| !seg.is_empty()) {
        if segments.push(segment).is_err() {
            segments.clear();
            return segments;
        }
    }
    segments
}

fn parse_action_status_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/actions/")?;
    let (action_id, leaf) = rest.split_once('/')?;
    if action_id.is_empty() || leaf != "status" {
        return None;
    }
    Some(action_id)
}

fn list_from_slice(
    entries: &[&str],
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    for entry in entries {
        push_list_entry(&mut output, entry)?;
    }
    Ok(output)
}

fn push_list_entry(
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    entry: &str,
) -> Result<(), NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    line.push_str(entry)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    output
        .push(line)
        .map_err(|_| NineDoorBridgeError::BufferFull)
}

fn lines_from_bytes(
    data: &[u8],
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let text = str::from_utf8(data).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    lines_from_text(text)
}

fn lines_from_text(
    text: &str,
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    script_lines(text)
}

fn validate_json_envelope(payload: &str) -> Result<(), NineDoorBridgeError> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn parse_action_lines(payload: &str) -> Result<Vec<PolicyAction>, NineDoorBridgeError> {
    let mut actions = Vec::new();
    for line in payload.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        actions.push(parse_action_line(trimmed)?);
    }
    Ok(actions)
}

fn parse_action_line(line: &str) -> Result<PolicyAction, NineDoorBridgeError> {
    let id = parse_json_string_field(line, "id").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let target =
        parse_json_string_field(line, "target").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let decision = parse_json_string_field(line, "decision")
        .ok_or(NineDoorBridgeError::InvalidPayload)?;
    validate_action_id(id)?;
    validate_action_target(target)?;
    let target = normalize_path(target);
    let decision = parse_policy_decision(decision)?;
    Ok(PolicyAction {
        id: String::from(id),
        target,
        decision,
        consumed: false,
    })
}

fn parse_policy_decision(value: &str) -> Result<PolicyDecision, NineDoorBridgeError> {
    match value {
        "approve" => Ok(PolicyDecision::Approve),
        "deny" => Ok(PolicyDecision::Deny),
        _ => Err(NineDoorBridgeError::InvalidPayload),
    }
}

fn validate_action_id(id: &str) -> Result<(), NineDoorBridgeError> {
    if id.is_empty() || id.len() > MAX_ACTION_ID_LEN {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_action_target(target: &str) -> Result<(), NineDoorBridgeError> {
    if !target.starts_with('/') {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let segments = split_path_segments(target);
    if segments.is_empty() {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let max_depth = generated::SECURE9P_LIMITS.walk_depth as usize;
    if segments.len() > max_depth {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    for segment in segments.iter() {
        if *segment == ".." || *segment == "*" || segment.is_empty() {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
    }
    Ok(())
}

fn path_matches_pattern(pattern: &str, path: &str) -> bool {
    let pattern_segments = split_path_segments(pattern);
    let path_segments = split_path_segments(path);
    if pattern_segments.len() != path_segments.len() {
        return false;
    }
    for (pattern_segment, path_segment) in
        pattern_segments.iter().zip(path_segments.iter())
    {
        if *pattern_segment == "*" {
            continue;
        }
        if *pattern_segment != *path_segment {
            return false;
        }
    }
    true
}

fn log_policy_action(role: &str, ticket: &str, action: &PolicyAction) {
    let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
    let _ = write!(
        line,
        "policy-action role={} ticket={} id={} decision={} target={}",
        role,
        ticket,
        action.id,
        action.decision.as_str(),
        action.target
    );
    log_buffer::append_log_line(line.as_str());
}

fn parse_worker_telemetry_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix(WORKER_ROOT)?;
    let (worker_id, leaf) = rest.split_once('/')?;
    if worker_id.is_empty() || leaf != WORKER_TELEMETRY_FILE {
        return None;
    }
    Some(worker_id)
}

fn append_log_bytes(
    log: &mut Vec<u8>,
    payload: &str,
    max_bytes: u32,
) -> Result<(), NineDoorBridgeError> {
    let payload_bytes = payload.as_bytes();
    let needs_newline = !payload_bytes.ends_with(b"\n");
    let extra = if needs_newline { 1 } else { 0 };
    let new_len = log
        .len()
        .saturating_add(payload_bytes.len())
        .saturating_add(extra);
    if new_len > max_bytes as usize {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    log.extend_from_slice(payload_bytes);
    if needs_newline {
        log.push(b'\n');
    }
    Ok(())
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
