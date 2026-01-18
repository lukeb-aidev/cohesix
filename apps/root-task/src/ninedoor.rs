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
use crate::observe::IngestSnapshot;
use crate::serial::DEFAULT_LINE_CAPACITY;
use alloc::{
    borrow::ToOwned,
    collections::{BTreeMap, VecDeque},
    format,
    string::String,
    vec::Vec,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use cohesix_cas::{CasManifest, CasManifestError, CAS_MANIFEST_SCHEMA};
use cohesix_ticket::TicketToken;
use ed25519_dalek::{Signature, SigningKey};
use core::fmt::{self, Write};
use core::str;
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use sidecar_bus::{LinkState, OfflineSpool, SpoolConfig, SpoolError, SpoolFrame};
use signature::Verifier;
use sha2::{Digest, Sha256};
use secure9p_codec::ErrorCode;
use worker_lora::{
    DutyCycleConfig, DutyCycleGuard, TamperEntry, TamperLog, TamperReason,
};

const LOG_PATH: &str = "/log/queen.log";
const QUEEN_CTL_PATH: &str = "/queen/ctl";
const BUS_ROOT_PATH: &str = "/bus";
const LORA_ROOT_PATH: &str = "/lora";
const PROC_BOOT_PATH: &str = "/proc/boot";
const PROC_TESTS_PATH: &str = "/proc/tests";
const PROC_TESTS_QUICK_PATH: &str = "/proc/tests/selftest_quick.coh";
const PROC_TESTS_FULL_PATH: &str = "/proc/tests/selftest_full.coh";
const PROC_TESTS_NEGATIVE_PATH: &str = "/proc/tests/selftest_negative.coh";
const PROC_INGEST_ROOT_PATH: &str = "/proc/ingest";
const PROC_INGEST_P50_PATH: &str = "/proc/ingest/p50_ms";
const PROC_INGEST_P95_PATH: &str = "/proc/ingest/p95_ms";
const PROC_INGEST_BACKPRESSURE_PATH: &str = "/proc/ingest/backpressure";
const PROC_INGEST_DROPPED_PATH: &str = "/proc/ingest/dropped";
const PROC_INGEST_QUEUED_PATH: &str = "/proc/ingest/queued";
const PROC_INGEST_WATCH_PATH: &str = "/proc/ingest/watch";
const BOOT_HEADER: &str = "Cohesix boot: root-task online";
const MAX_STREAM_LINES: usize = log_buffer::LOG_SNAPSHOT_LINES;
const MAX_WORKERS: usize = 8;
const MAX_BINDS: usize = 8;
const CAS_MAX_CHUNKS: usize = 8;
const CAS_MAX_UPDATES: usize = 8;
const CAS_MAX_MODELS: usize = 8;
const CAS_QUARANTINE_LIMIT: usize = 8;
const CAS_MANIFEST_MAX_BYTES: usize = 2048;
const MAX_EPOCH_LEN: usize = 20;
const UI_MAX_STREAM_BYTES: usize = 32 * 1024;
const MAX_WORKER_ID_LEN: usize = 32;
const TELEMETRY_AUDIT_LINE: usize = 128;
const WORKER_TELEMETRY_FILE: &str = "telemetry";
const POLICY_CTL_PATH: &str = "/policy/ctl";
const POLICY_RULES_PATH: &str = "/policy/rules";
const POLICY_ROOT_PATH: &str = "/policy";
const POLICY_PREFLIGHT_ROOT_PATH: &str = "/policy/preflight";
const POLICY_PREFLIGHT_REQ_PATH: &str = "/policy/preflight/req";
const POLICY_PREFLIGHT_REQ_CBOR_PATH: &str = "/policy/preflight/req.cbor";
const POLICY_PREFLIGHT_DIFF_PATH: &str = "/policy/preflight/diff";
const POLICY_PREFLIGHT_DIFF_CBOR_PATH: &str = "/policy/preflight/diff.cbor";
const ACTIONS_QUEUE_PATH: &str = "/actions/queue";
const ACTIONS_ROOT_PATH: &str = "/actions";
const AUDIT_ROOT_PATH: &str = "/audit";
const AUDIT_JOURNAL_PATH: &str = "/audit/journal";
const AUDIT_DECISIONS_PATH: &str = "/audit/decisions";
const AUDIT_EXPORT_PATH: &str = "/audit/export";
const REPLAY_ROOT_PATH: &str = "/replay";
const REPLAY_CTL_PATH: &str = "/replay/ctl";
const REPLAY_STATUS_PATH: &str = "/replay/status";
const MAX_POLICY_PATH_COMPONENTS: usize = 8;
const MAX_ACTION_ID_LEN: usize = 64;
const SYSTEMD_UNITS: [&str; 2] = ["cohesix-agent.service", "ssh.service"];
const K8S_NODES: [&str; 1] = ["node-1"];
const NVIDIA_GPUS: [&str; 1] = ["0"];
const OBSERVE_P50_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.p50_ms_bytes as usize;
const OBSERVE_P95_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.p95_ms_bytes as usize;
const OBSERVE_BACKPRESSURE_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.backpressure_bytes as usize;
const OBSERVE_DROPPED_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.dropped_bytes as usize;
const OBSERVE_QUEUED_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.queued_bytes as usize;
const OBSERVE_WATCH_MAX_ENTRIES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.watch_max_entries as usize;
const OBSERVE_WATCH_LINE_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.watch_line_bytes as usize;
const OBSERVE_WATCH_MIN_INTERVAL_MS: u64 =
    generated::OBSERVABILITY_CONFIG.proc_ingest.watch_min_interval_ms as u64;
const SIDECAR_LOG_MAX_BYTES: usize = generated::SECURE9P_LIMITS.msize as usize;

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
    session_scope: Option<String>,
    next_worker_id: u32,
    ui: generated::UiProviderConfig,
    telemetry: generated::TelemetryConfig,
    workers: HeaplessVec<WorkerTelemetry, MAX_WORKERS>,
    binds: HeaplessVec<BindEntry, MAX_BINDS>,
    host: HostState,
    sidecars: SidecarState,
    policy: PolicyState,
    audit: AuditState,
    replay: ReplayState,
    observe: ObserveState,
    cas: CasState,
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
            session_scope: None,
            next_worker_id: 1,
            ui: generated::ui_provider_config(),
            telemetry: generated::telemetry_config(),
            workers: HeaplessVec::new(),
            binds: HeaplessVec::new(),
            host: HostState::new(),
            sidecars: SidecarState::new(),
            policy: PolicyState::new(),
            audit: AuditState::new(generated::audit_config()),
            replay: ReplayState::new(generated::audit_config()),
            observe: ObserveState::new(),
            cas: CasState::new(generated::cas_config()),
        }
    }

    /// Reset per-session state after a console disconnect.
    pub fn reset_session(&mut self) {
        self.attached = false;
        self.session_role = None;
        self.session_ticket = None;
        self.session_scope = None;
        self.binds.clear();
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

    /// Emit lines for `/proc/ingest/watch` with throttling applied.
    pub fn ingest_watch_lines(
        &mut self,
        now_ms: u64,
        audit: &mut dyn AuditSink,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        self.observe.watch_lines(now_ms, audit)
    }

    /// Handle a log stream request.
    pub fn log_stream(&mut self, audit: &mut dyn AuditSink) -> Result<(), NineDoorBridgeError> {
        audit.info("nine-door: log stream requested");
        Ok(())
    }

    /// Update the most recent ingest snapshot from the event pump.
    pub fn update_ingest_snapshot(&mut self, snapshot: IngestSnapshot) {
        self.observe.update_ingest_snapshot(snapshot);
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
        let result = self.handle_queen_ctl(payload);
        if self.audit.enabled {
            let outcome = ControlOutcome::from_result(&result);
            let role = self.role_label();
            let ticket = String::from(self.ticket_label());
            self.audit.record_control(
                QUEEN_CTL_PATH,
                payload,
                outcome,
                role,
                ticket.as_str(),
            )?;
        }
        result
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
        let payload = format!("{{\"kill\":\"{}\"}}", escape_json_string(identifier));
        let result = self.remove_worker(identifier);
        if self.audit.enabled {
            let outcome = ControlOutcome::from_result(&result);
            let role = self.role_label();
            let ticket = String::from(self.ticket_label());
            self.audit.record_control(
                QUEEN_CTL_PATH,
                payload.as_str(),
                outcome,
                role,
                ticket.as_str(),
            )?;
        }
        result
    }

    /// Append a payload line to an append-only file.
    pub fn echo(&mut self, path: &str, payload: &str) -> Result<(), NineDoorBridgeError> {
        if payload.contains('\n') || payload.contains('\r') {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let segments = split_path_segments(path);
        if path == LOG_PATH {
            log_buffer::append_user_line(payload);
            log_buffer::append_log_line(payload);
            return Ok(());
        }
        if self.audit.enabled {
            if path == AUDIT_JOURNAL_PATH {
                self.audit.append_manual_journal(payload)?;
                return Ok(());
            }
            if path == AUDIT_DECISIONS_PATH || path == AUDIT_EXPORT_PATH {
                return Err(NineDoorBridgeError::Permission);
            }
        }
        if self.replay.enabled {
            if path == REPLAY_CTL_PATH {
                self.replay
                    .handle_ctl(payload, &mut self.audit)?;
                return Ok(());
            }
            if path == REPLAY_STATUS_PATH {
                return Err(NineDoorBridgeError::Permission);
            }
        }
        if self.policy.enabled {
            if path == POLICY_CTL_PATH {
                self.policy.append_policy_ctl(payload)?;
                return Ok(());
            }
            if path == ACTIONS_QUEUE_PATH {
                let role = self.role_label();
                let ticket = String::from(self.ticket_label());
                let before = self.policy.actions.len();
                self.policy
                    .append_action_queue(payload, role, ticket.as_str())?;
                if self.audit.enabled {
                    for action in self.policy.actions.iter().skip(before) {
                        self.audit.record_decision_action(action, role, ticket.as_str())?;
                    }
                }
                return Ok(());
            }
        }
        if path == QUEEN_CTL_PATH {
            let role = self.role_label();
            let ticket = String::from(self.ticket_label());
            let decision = self.apply_policy_gate(path)?;
            match decision {
                PolicyGateDecision::Denied(_) => {
                    if self.audit.enabled {
                        self.audit.record_control(
                            path,
                            payload,
                            ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                            role,
                            ticket.as_str(),
                        )?;
                    }
                    return Err(NineDoorBridgeError::Permission);
                }
                PolicyGateDecision::Allowed(_) => {}
            }
            let result = self.handle_queen_ctl(payload);
            if self.audit.enabled {
                let outcome = ControlOutcome::from_result(&result);
                self.audit
                    .record_control(path, payload, outcome, role, ticket.as_str())?;
            }
            return result;
        }
        if let Some(control) = self.host.control_label(path) {
            if !self.is_queen() {
                self.log_host_write(path, Some(control), HostWriteOutcome::Denied, None);
                if self.audit.enabled {
                    let role = self.role_label();
                    let ticket = String::from(self.ticket_label());
                    self.audit.record_control(
                        path,
                        payload,
                        ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                        role,
                        ticket.as_str(),
                    )?;
                }
                return Err(NineDoorBridgeError::Permission);
            }
            let role = self.role_label();
            let ticket = String::from(self.ticket_label());
            let decision = self.apply_policy_gate(path)?;
            match decision {
                PolicyGateDecision::Denied(_) => {
                    if self.audit.enabled {
                        self.audit.record_control(
                            path,
                            payload,
                            ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                            role,
                            ticket.as_str(),
                        )?;
                    }
                    return Err(NineDoorBridgeError::Permission);
                }
                PolicyGateDecision::Allowed(_) => {}
            }
            self.host.update_value(path, payload);
            self.log_host_write(
                path,
                Some(control),
                HostWriteOutcome::Allowed,
                Some(payload.len()),
            );
            if self.audit.enabled {
                self.audit.record_control(
                    path,
                    payload,
                    ControlOutcome::ok(),
                    role,
                    ticket.as_str(),
                )?;
            }
            return Ok(());
        }
        if self.host.entry_value(path).is_some() {
            self.log_host_write(path, None, HostWriteOutcome::Denied, None);
            if self.audit.enabled {
                let role = self.role_label();
                let ticket = String::from(self.ticket_label());
                self.audit.record_control(
                    path,
                    payload,
                    ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                    role,
                    ticket.as_str(),
                )?;
            }
            return Err(NineDoorBridgeError::Permission);
        }
        if let Some(kind) = self.sidecars.kind_for_path(segments.as_slice()) {
            if !self.sidecar_allowed(kind, segments.as_slice(), SidecarAccess::Write) {
                self.log_sidecar_denial(kind);
                return Err(NineDoorBridgeError::Permission);
            }
            if self
                .sidecars
                .write(segments.as_slice(), payload.as_bytes())?
                .is_some()
            {
                return Ok(());
            }
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let resolved = self.resolve_bound_path(path);
        let resolved_path = resolved.as_deref().unwrap_or(path);
        if let Some(outcome) = self
            .cas
            .append_path(resolved_path, payload.as_bytes(), self.is_queen())?
        {
            return Ok(outcome);
        }
        if let Some(worker_id) = parse_worker_telemetry_path(resolved_path) {
            return self.append_worker_telemetry(worker_id, payload.as_bytes());
        }
        Err(NineDoorBridgeError::InvalidPath)
    }

    /// Read file contents as line-oriented output.
    pub fn cat(
        &mut self,
        path: &str,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let segments = split_path_segments(path);
        if path == LOG_PATH {
            return Ok(log_buffer::snapshot_lines::<
                DEFAULT_LINE_CAPACITY,
                MAX_STREAM_LINES,
            >());
        }
        if self.audit.enabled {
            if path == AUDIT_JOURNAL_PATH {
                return lines_from_bytes(&self.audit.journal_snapshot());
            }
            if path == AUDIT_DECISIONS_PATH {
                return lines_from_bytes(&self.audit.decisions_snapshot());
            }
            if path == AUDIT_EXPORT_PATH {
                return lines_from_bytes(&self.audit.export_snapshot());
            }
        }
        if self.replay.enabled {
            if path == REPLAY_CTL_PATH {
                return lines_from_bytes(self.replay.ctl_log());
            }
            if path == REPLAY_STATUS_PATH {
                return lines_from_bytes(self.replay.status());
            }
        }
        if self.policy.enabled {
            if self.ui.policy_preflight.req && path == POLICY_PREFLIGHT_REQ_PATH {
                let payload = self.policy.preflight_req_text()?;
                return lines_from_bytes(payload.as_slice());
            }
            if self.ui.policy_preflight.req && path == POLICY_PREFLIGHT_REQ_CBOR_PATH {
                let payload = self.policy.preflight_req_cbor()?;
                return cas_lines_from_bytes(payload.as_slice());
            }
            if self.ui.policy_preflight.diff && path == POLICY_PREFLIGHT_DIFF_PATH {
                let payload = self.policy.preflight_diff_text()?;
                return lines_from_bytes(payload.as_slice());
            }
            if self.ui.policy_preflight.diff && path == POLICY_PREFLIGHT_DIFF_CBOR_PATH {
                let payload = self.policy.preflight_diff_cbor()?;
                return cas_lines_from_bytes(payload.as_slice());
            }
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
        if let Some(kind) = self.sidecars.kind_for_path(segments.as_slice()) {
            if !self.sidecar_allowed(kind, segments.as_slice(), SidecarAccess::Read) {
                self.log_sidecar_denial(kind);
                return Err(NineDoorBridgeError::Permission);
            }
            if let Some(data) = self.sidecars.read(segments.as_slice()) {
                return lines_from_bytes(&data);
            }
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let resolved = self.resolve_bound_path(path);
        let path = resolved.as_deref().unwrap_or(path);
        if let Some(CasPath::UpdateStatus { epoch, cbor }) = parse_cas_path(path)? {
            if !self.is_queen() {
                return Err(NineDoorBridgeError::Permission);
            }
            if !self.cas.enabled() || !self.ui.updates.status {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            let payloads = self.cas.update_status_payloads(epoch.as_str())?;
            if cbor {
                return cas_lines_from_bytes(payloads.cbor.as_slice());
            }
            return lines_from_bytes(payloads.text.as_slice());
        }
        if let Some(bytes) = self.cas.read_path(path, self.is_queen())? {
            return cas_lines_from_bytes(&bytes);
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
        if let Some(result) = self.observe.ingest_lines(path) {
            return result;
        }
        Err(NineDoorBridgeError::InvalidPath)
    }

    /// List directory entries (not yet supported by the shim bridge).
    pub fn list(
        &mut self,
        path: &str,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let sharding = generated::sharding_config();
        let segments = split_path_segments(path);
        if path == "/worker" {
            if sharding.enabled && !legacy_worker_alias_enabled(sharding) {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return self.list_workers();
        }
        if path == "/shard" {
            if !sharding.enabled {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return list_shard_labels();
        }
        if let Some((label, worker_root)) = parse_shard_worker_root(path) {
            if !sharding.enabled || !shard_label_known(label) {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if worker_root {
                return self.list_workers_for_shard(label);
            }
            return list_from_slice(&["worker"]);
        }
        if path == "/" {
            let mut output = HeaplessVec::new();
            for entry in ["gpu", "kmesg", "log", "proc", "queen", "trace"] {
                push_list_entry(&mut output, entry)?;
            }
            if self.cas.enabled() {
                push_list_entry(&mut output, "updates")?;
                if self.cas.models_enabled() {
                    push_list_entry(&mut output, "models")?;
                }
            }
            if sharding.enabled {
                push_list_entry(&mut output, "shard")?;
            }
            if !sharding.enabled || legacy_worker_alias_enabled(sharding) {
                push_list_entry(&mut output, "worker")?;
            }
            if self.host.enabled {
                push_list_entry(&mut output, self.host.mount_label())?;
            }
            self.sidecars.push_root_entries(&mut output)?;
            if self.policy.enabled {
                push_list_entry(&mut output, "policy")?;
                push_list_entry(&mut output, "actions")?;
            }
            if self.audit.enabled {
                push_list_entry(&mut output, "audit")?;
            }
            if self.replay.enabled {
                push_list_entry(&mut output, "replay")?;
            }
            return Ok(output);
        }
        if path == "/log" {
            return list_from_slice(&["queen.log"]);
        }
        if path == "/proc" {
            let mut output = HeaplessVec::new();
            push_list_entry(&mut output, "boot")?;
            push_list_entry(&mut output, "tests")?;
            if self.observe.proc_ingest_enabled() {
                push_list_entry(&mut output, "ingest")?;
            }
            return Ok(output);
        }
        if path == "/proc/tests" {
            return list_from_slice(&[
                "selftest_quick.coh",
                "selftest_full.coh",
                "selftest_negative.coh",
            ]);
        }
        if path == PROC_INGEST_ROOT_PATH {
            return self.observe.list_ingest();
        }
        if path == "/queen" {
            return list_from_slice(&["ctl"]);
        }
        if path == "/trace" {
            return list_from_slice(&["ctl", "events"]);
        }
        if path == "/gpu" {
            return Ok(HeaplessVec::new());
        }
        if path == "/worker" {
            if sharding.enabled && !legacy_worker_alias_enabled(sharding) {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return Ok(HeaplessVec::new());
        }
        if self.policy.enabled {
            if path == POLICY_ROOT_PATH {
                let mut output = HeaplessVec::new();
                push_list_entry(&mut output, "ctl")?;
                push_list_entry(&mut output, "rules")?;
                if self.ui.policy_preflight.req || self.ui.policy_preflight.diff {
                    push_list_entry(&mut output, "preflight")?;
                }
                return Ok(output);
            }
            if path == POLICY_PREFLIGHT_ROOT_PATH {
                if !self.ui.policy_preflight.req && !self.ui.policy_preflight.diff {
                    return Err(NineDoorBridgeError::InvalidPath);
                }
                let mut output = HeaplessVec::new();
                if self.ui.policy_preflight.req {
                    push_list_entry(&mut output, "req")?;
                    push_list_entry(&mut output, "req.cbor")?;
                }
                if self.ui.policy_preflight.diff {
                    push_list_entry(&mut output, "diff")?;
                    push_list_entry(&mut output, "diff.cbor")?;
                }
                return Ok(output);
            }
            if path == ACTIONS_ROOT_PATH {
                return list_from_slice(&["queue"]);
            }
        }
        if self.audit.enabled && path == AUDIT_ROOT_PATH {
            return list_from_slice(&["journal", "decisions", "export"]);
        }
        if self.replay.enabled && path == REPLAY_ROOT_PATH {
            return list_from_slice(&["ctl", "status"]);
        }
        if let Some(kind) = self.sidecars.kind_for_path(segments.as_slice()) {
            if !self.sidecar_allowed(kind, segments.as_slice(), SidecarAccess::List) {
                self.log_sidecar_denial(kind);
                return Err(NineDoorBridgeError::Permission);
            }
            if let Some(output) = self.sidecars.list(segments.as_slice()) {
                return output;
            }
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let resolved = self.resolve_bound_path(path);
        let path = resolved.as_deref().unwrap_or(path);
        if let Some(output) = self.cas.list_path(
            path,
            self.is_queen(),
            self.ui.updates.manifest,
            self.ui.updates.status,
        )? {
            return Ok(output);
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

    fn list_workers_for_shard(
        &self,
        label: &str,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let sharding = generated::sharding_config();
        let mut output = HeaplessVec::new();
        for worker in self.workers.iter() {
            let worker_label = worker_shard_label(worker.id.as_str(), sharding);
            if worker_label == label {
                push_list_entry(&mut output, worker.id.as_str())?;
            }
        }
        Ok(output)
    }

    fn handle_queen_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        let command = parse_queen_ctl(payload)?;
        match command {
            QueenCtlCommand::Spawn(target) => self.spawn_worker(target),
            QueenCtlCommand::Kill(worker_id) => self.remove_worker(worker_id),
            QueenCtlCommand::Bind { from, to } => self.bind_namespace(from, to),
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

    fn bind_namespace(&mut self, from: &str, to: &str) -> Result<(), NineDoorBridgeError> {
        validate_bind_path(from)?;
        validate_bind_path(to)?;
        let from = normalize_path(from);
        let to = normalize_path(to);
        if let Some(existing) = self.binds.iter_mut().find(|entry| entry.to == to) {
            existing.from = from;
            return Ok(());
        }
        self.binds
            .push(BindEntry { from, to })
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        Ok(())
    }

    fn resolve_bound_path(&self, path: &str) -> Option<String> {
        if self.binds.is_empty() {
            return None;
        }
        let normalized = normalize_path(path);
        let mut best: Option<&BindEntry> = None;
        let mut best_len = 0usize;
        for entry in self.binds.iter() {
            let to = entry.to.as_str();
            if normalized == to {
                if to.len() > best_len {
                    best = Some(entry);
                    best_len = to.len();
                }
                continue;
            }
            if normalized.starts_with(to) {
                let remainder = &normalized[to.len()..];
                if remainder.starts_with('/') && to.len() > best_len {
                    best = Some(entry);
                    best_len = to.len();
                }
            }
        }
        let entry = best?;
        let to = entry.to.as_str();
        if normalized == to {
            return Some(entry.from.clone());
        }
        let remainder = &normalized[to.len()..];
        let mut out = String::new();
        out.push_str(entry.from.as_str());
        out.push_str(remainder);
        Some(out)
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
        self.session_scope = None;
        if matches!(
            self.session_role,
            Some(SessionRoleLabel::WorkerBus | SessionRoleLabel::WorkerLora)
        ) {
            if let Some(ticket) = ticket {
                if let Ok(claims) = TicketToken::decode_unverified(ticket) {
                    self.session_scope = claims.subject;
                }
            }
        }
    }

    fn role_label(&self) -> &'static str {
        match self.session_role {
            Some(SessionRoleLabel::Queen) => "queen",
            Some(SessionRoleLabel::WorkerHeartbeat) => "worker-heartbeat",
            Some(SessionRoleLabel::WorkerGpu) => "worker-gpu",
            Some(SessionRoleLabel::WorkerBus) => "worker-bus",
            Some(SessionRoleLabel::WorkerLora) => "worker-lora",
            None => "unauthenticated",
        }
    }

    fn ticket_label(&self) -> &str {
        self.session_ticket.as_deref().unwrap_or("none")
    }

    fn is_queen(&self) -> bool {
        matches!(self.session_role, Some(SessionRoleLabel::Queen))
    }

    fn session_scope(&self) -> Option<&str> {
        self.session_scope.as_deref()
    }

    fn sidecar_role(&self) -> Option<SidecarKind> {
        match self.session_role {
            Some(SessionRoleLabel::WorkerBus) => Some(SidecarKind::Bus),
            Some(SessionRoleLabel::WorkerLora) => Some(SidecarKind::Lora),
            _ => None,
        }
    }

    fn log_sidecar_denial(&self, kind: SidecarKind) {
        let scope = self.session_scope().unwrap_or("none");
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(line, "sidecar-deny kind={} scope={}", kind.as_str(), scope);
        log_buffer::append_log_line(line.as_str());
    }

    fn sidecar_allowed(&self, kind: SidecarKind, path: &[&str], access: SidecarAccess) -> bool {
        if self.is_queen() {
            return true;
        }
        if self.sidecar_role() != Some(kind) {
            return false;
        }
        let scope = self.session_scope();
        match access {
            SidecarAccess::List | SidecarAccess::Read => {
                self.sidecars.allowed_prefix(kind, scope, path)
            }
            SidecarAccess::Write => self.sidecars.allowed_path(kind, scope, path),
        }
    }

    fn apply_policy_gate(
        &mut self,
        path: &str,
    ) -> Result<PolicyGateDecision, NineDoorBridgeError> {
        let decision = self.policy.consume_gate(path);
        match &decision {
            PolicyGateDecision::Allowed(allowance) => {
                if matches!(allowance, PolicyGateAllowance::Action { .. }) {
                    self.log_policy_gate_allow(path, allowance);
                }
                if self.audit.enabled {
                    let role = self.role_label();
                    let ticket = String::from(self.ticket_label());
                    self.audit
                        .record_decision_gate(path, allowance, role, ticket.as_str())?;
                }
            }
            PolicyGateDecision::Denied(denial) => {
                self.log_policy_gate_deny(path, denial);
                if self.audit.enabled {
                    let role = self.role_label();
                    let ticket = String::from(self.ticket_label());
                    self.audit
                        .record_decision_gate_denial(path, denial, role, ticket.as_str())?;
                }
            }
        }
        Ok(decision)
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
    WorkerBus,
    WorkerLora,
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
    } else if role.eq_ignore_ascii_case("worker-bus") {
        Some(SessionRoleLabel::WorkerBus)
    } else if role.eq_ignore_ascii_case("worker-lora") {
        Some(SessionRoleLabel::WorkerLora)
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
struct ObserveState {
    proc_ingest: generated::ProcIngestConfig,
    snapshot: IngestSnapshot,
    watch: IngestWatch,
}

impl ObserveState {
    fn new() -> Self {
        let config = generated::observability_config();
        Self {
            proc_ingest: config.proc_ingest,
            snapshot: IngestSnapshot::default(),
            watch: IngestWatch::new(),
        }
    }

    fn proc_ingest_enabled(&self) -> bool {
        self.proc_ingest.p50_ms
            || self.proc_ingest.p95_ms
            || self.proc_ingest.backpressure
            || self.proc_ingest.dropped
            || self.proc_ingest.queued
            || self.proc_ingest.watch
    }

    fn update_ingest_snapshot(&mut self, snapshot: IngestSnapshot) {
        self.snapshot = snapshot;
    }

    fn ingest_lines(
        &self,
        path: &str,
    ) -> Option<
        Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>,
    > {
        match path {
            PROC_INGEST_P50_PATH if self.proc_ingest.p50_ms => Some(
                render_p50_line(self.snapshot)
                    .and_then(|line| lines_from_text(line.as_str())),
            ),
            PROC_INGEST_P95_PATH if self.proc_ingest.p95_ms => Some(
                render_p95_line(self.snapshot)
                    .and_then(|line| lines_from_text(line.as_str())),
            ),
            PROC_INGEST_BACKPRESSURE_PATH if self.proc_ingest.backpressure => Some(
                render_backpressure_line(self.snapshot)
                    .and_then(|line| lines_from_text(line.as_str())),
            ),
            PROC_INGEST_DROPPED_PATH if self.proc_ingest.dropped => Some(
                render_dropped_line(self.snapshot)
                    .and_then(|line| lines_from_text(line.as_str())),
            ),
            PROC_INGEST_QUEUED_PATH if self.proc_ingest.queued => Some(
                render_queued_line(self.snapshot)
                    .and_then(|line| lines_from_text(line.as_str())),
            ),
            PROC_INGEST_WATCH_PATH if self.proc_ingest.watch => Some(self.watch.lines()),
            _ => None,
        }
    }

    fn watch_lines(
        &mut self,
        now_ms: u64,
        audit: &mut dyn AuditSink,
    ) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
    {
        if !self.proc_ingest.watch {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        self.watch.maybe_append(now_ms, self.snapshot, audit)?;
        self.watch.lines()
    }

    fn list_ingest(
        &self,
    ) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
    {
        if !self.proc_ingest_enabled() {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let mut output = HeaplessVec::new();
        if self.proc_ingest.p50_ms {
            push_list_entry(&mut output, "p50_ms")?;
        }
        if self.proc_ingest.p95_ms {
            push_list_entry(&mut output, "p95_ms")?;
        }
        if self.proc_ingest.backpressure {
            push_list_entry(&mut output, "backpressure")?;
        }
        if self.proc_ingest.dropped {
            push_list_entry(&mut output, "dropped")?;
        }
        if self.proc_ingest.queued {
            push_list_entry(&mut output, "queued")?;
        }
        if self.proc_ingest.watch {
            push_list_entry(&mut output, "watch")?;
        }
        Ok(output)
    }
}

#[derive(Debug)]
struct IngestWatch {
    entries: HeaplessVec<HeaplessString<OBSERVE_WATCH_LINE_BYTES>, OBSERVE_WATCH_MAX_ENTRIES>,
    last_emit_ms: Option<u64>,
}

impl IngestWatch {
    fn new() -> Self {
        Self {
            entries: HeaplessVec::new(),
            last_emit_ms: None,
        }
    }

    fn maybe_append(
        &mut self,
        now_ms: u64,
        snapshot: IngestSnapshot,
        audit: &mut dyn AuditSink,
    ) -> Result<(), NineDoorBridgeError> {
        if OBSERVE_WATCH_MAX_ENTRIES == 0 || OBSERVE_WATCH_LINE_BYTES == 0 {
            return Ok(());
        }
        if let Some(last) = self.last_emit_ms {
            let next_ok = last.saturating_add(OBSERVE_WATCH_MIN_INTERVAL_MS);
            if now_ms < next_ok {
                let delay_ms = next_ok.saturating_sub(now_ms);
                log_watch_throttle(audit, delay_ms);
                return Ok(());
            }
        }
        let mut line = HeaplessString::new();
        write!(
            line,
            "watch ts_ms={} p50_ms={} p95_ms={} queued={} backpressure={} dropped={} ui_reads={} ui_denies={}",
            now_ms,
            snapshot.p50_ms,
            snapshot.p95_ms,
            snapshot.queued,
            snapshot.backpressure,
            snapshot.dropped,
            snapshot.ui_reads,
            snapshot.ui_denies
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        if self.entries.is_full() {
            let _ = self.entries.remove(0);
        }
        self.entries
            .push(line)
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        self.last_emit_ms = Some(now_ms);
        Ok(())
    }

    fn lines(
        &self,
    ) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
    {
        let mut output = HeaplessVec::new();
        for entry in self.entries.iter() {
            let mut line = HeaplessString::new();
            line.push_str(entry.as_str())
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
            output
                .push(line)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        Ok(output)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelFileKind {
    Weights,
    Schema,
    Signature,
}

#[derive(Debug, Clone)]
enum CasPath {
    UpdatesRoot,
    UpdateEpoch { epoch: String },
    UpdateManifest { epoch: String },
    UpdateStatus { epoch: String, cbor: bool },
    UpdateChunks { epoch: String },
    UpdateChunk { epoch: String, digest: [u8; 32] },
    ModelsRoot,
    ModelRoot { digest: [u8; 32] },
    ModelFile { digest: [u8; 32], kind: ModelFileKind },
}

#[derive(Debug)]
struct CasState {
    config: generated::CasConfig,
    updates: BTreeMap<String, UpdateBundle>,
    chunks: BTreeMap<[u8; 32], Vec<u8>>,
    pending_chunks: BTreeMap<[u8; 32], Vec<u8>>,
    models: BTreeMap<[u8; 32], ModelBundle>,
    quarantine: VecDeque<QuarantineEntry>,
    bytes_used: usize,
}

impl CasState {
    fn new(config: generated::CasConfig) -> Self {
        Self {
            config,
            updates: BTreeMap::new(),
            chunks: BTreeMap::new(),
            pending_chunks: BTreeMap::new(),
            models: BTreeMap::new(),
            quarantine: VecDeque::new(),
            bytes_used: 0,
        }
    }

    fn enabled(&self) -> bool {
        self.config.enable
    }

    fn models_enabled(&self) -> bool {
        self.config.enable && self.config.models_enabled
    }

    fn append_path(
        &mut self,
        path: &str,
        payload: &[u8],
        is_queen: bool,
    ) -> Result<Option<()>, NineDoorBridgeError> {
        let Some(cas_path) = parse_cas_path(path)? else {
            return Ok(None);
        };
        if !is_queen {
            return Err(NineDoorBridgeError::Permission);
        }
        match cas_path {
            CasPath::UpdateManifest { epoch } => {
                let _ = self.append_manifest(&epoch, u64::MAX, payload)?;
                Ok(Some(()))
            }
            CasPath::UpdateChunk { epoch, digest } => {
                let _ = self.append_chunk(&epoch, &digest, u64::MAX, payload)?;
                Ok(Some(()))
            }
            CasPath::ModelFile { digest, kind } => {
                let _ = self.append_model_file(&digest, kind, u64::MAX, payload)?;
                Ok(Some(()))
            }
            _ => Err(NineDoorBridgeError::InvalidPath),
        }
    }

    fn read_path(
        &mut self,
        path: &str,
        is_queen: bool,
    ) -> Result<Option<Vec<u8>>, NineDoorBridgeError> {
        let Some(cas_path) = parse_cas_path(path)? else {
            return Ok(None);
        };
        if !is_queen {
            return Err(NineDoorBridgeError::Permission);
        }
        let data = match cas_path {
            CasPath::UpdateManifest { epoch } => self.read_manifest(&epoch)?,
            CasPath::UpdateChunk { digest, .. } => self.read_chunk(&digest)?,
            CasPath::ModelFile { digest, kind } => self.read_model_file(&digest, kind)?,
            _ => return Err(NineDoorBridgeError::InvalidPath),
        };
        Ok(Some(data))
    }

    fn list_path(
        &mut self,
        path: &str,
        is_queen: bool,
        ui_updates_manifest: bool,
        ui_updates_status: bool,
    ) -> Result<
        Option<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>>,
        NineDoorBridgeError,
    > {
        let Some(cas_path) = parse_cas_path(path)? else {
            return Ok(None);
        };
        if !is_queen {
            return Err(NineDoorBridgeError::Permission);
        }
        let entries = match cas_path {
            CasPath::UpdatesRoot => {
                self.ensure_enabled()?;
                self.list_updates()
            }
            CasPath::UpdateEpoch { epoch } => {
                self.ensure_update(&epoch)?;
                let mut entries = Vec::new();
                entries.push("chunks".to_owned());
                if ui_updates_manifest {
                    entries.push("manifest.cbor".to_owned());
                }
                if ui_updates_status {
                    entries.push("status".to_owned());
                    entries.push("status.cbor".to_owned());
                }
                entries
            }
            CasPath::UpdateChunks { epoch } => {
                self.ensure_update(&epoch)?;
                self.list_update_chunks(&epoch)
            }
            CasPath::ModelsRoot => {
                self.ensure_models_enabled()?;
                self.list_models()
            }
            CasPath::ModelRoot { digest } => {
                self.ensure_model_entry(&digest)?;
                self.list_model_entries(&digest)
            }
            _ => return Err(NineDoorBridgeError::InvalidPath),
        };
        let mut output = HeaplessVec::new();
        for entry in entries {
            push_list_entry(&mut output, entry.as_str())?;
        }
        Ok(Some(output))
    }

    fn read_manifest(&self, epoch: &str) -> Result<Vec<u8>, NineDoorBridgeError> {
        let bundle = self.updates.get(epoch).ok_or(NineDoorBridgeError::InvalidPath)?;
        let data = bundle
            .manifest_bytes
            .as_deref()
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        Ok(data.to_vec())
    }

    fn read_chunk(&self, digest: &[u8; 32]) -> Result<Vec<u8>, NineDoorBridgeError> {
        let data = self.chunks.get(digest).ok_or(NineDoorBridgeError::InvalidPath)?;
        Ok(data.clone())
    }

    fn read_model_file(
        &self,
        digest: &[u8; 32],
        kind: ModelFileKind,
    ) -> Result<Vec<u8>, NineDoorBridgeError> {
        let model = self.models.get(digest).ok_or(NineDoorBridgeError::InvalidPath)?;
        match kind {
            ModelFileKind::Weights => self.read_chunk(digest),
            ModelFileKind::Schema => model
                .schema
                .as_deref()
                .map(|data| data.to_vec())
                .ok_or(NineDoorBridgeError::InvalidPath),
            ModelFileKind::Signature => model
                .signature
                .as_deref()
                .map(|data| data.to_vec())
                .ok_or(NineDoorBridgeError::InvalidPath),
        }
    }

    fn update_status_payloads(
        &self,
        epoch: &str,
    ) -> Result<UpdateStatusPayloads, NineDoorBridgeError> {
        let snapshot = self.update_status_snapshot(epoch)?;
        let text = build_update_status_text(&snapshot)?;
        let cbor = build_update_status_cbor(&snapshot)?;
        Ok(UpdateStatusPayloads { text, cbor })
    }

    fn update_status_snapshot(
        &self,
        epoch: &str,
    ) -> Result<UpdateStatusSnapshot, NineDoorBridgeError> {
        let bundle = self
            .updates
            .get(epoch)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let manifest_bytes = bundle.manifest_bytes.as_ref().map_or(0, |data| data.len());
        let manifest_pending_bytes = bundle.manifest_pending.len();
        let mut snapshot = UpdateStatusSnapshot {
            epoch: epoch.to_owned(),
            state: "empty",
            manifest_bytes,
            manifest_pending_bytes,
            chunks_expected: 0,
            chunks_committed: 0,
            chunks_pending: 0,
            chunks_missing: 0,
            payload_bytes: 0,
            payload_sha256: None,
            delta_base_epoch: None,
            delta_base_sha256: None,
        };
        let Some(manifest) = bundle.manifest.as_ref() else {
            if manifest_pending_bytes > 0 {
                snapshot.state = "manifest_pending";
            }
            return Ok(snapshot);
        };
        snapshot.payload_bytes = manifest.payload_bytes;
        snapshot.payload_sha256 = Some(manifest.payload_sha256);
        if let Some(delta) = &manifest.delta {
            snapshot.delta_base_epoch = Some(delta.base_epoch.clone());
            snapshot.delta_base_sha256 = Some(delta.base_sha256);
        }
        snapshot.chunks_expected = manifest.chunks.len();
        for digest in &manifest.chunks {
            if self.chunks.contains_key(digest) {
                snapshot.chunks_committed = snapshot.chunks_committed.saturating_add(1);
                continue;
            }
            if self.pending_chunks.contains_key(digest) {
                snapshot.chunks_pending = snapshot.chunks_pending.saturating_add(1);
            }
        }
        snapshot.chunks_missing = snapshot
            .chunks_expected
            .saturating_sub(snapshot.chunks_committed)
            .saturating_sub(snapshot.chunks_pending);
        if snapshot.chunks_expected == snapshot.chunks_committed {
            snapshot.state = "ready";
        } else {
            snapshot.state = "chunks_pending";
        }
        Ok(snapshot)
    }

    fn append_manifest(
        &mut self,
        epoch: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorBridgeError> {
        self.ensure_enabled()?;
        self.ensure_update(epoch)?;
        let (decoded, manifest_bytes) = {
            let bundle = self
                .updates
                .get_mut(epoch)
                .expect("update bundle must exist");
            if bundle.manifest_bytes.is_some() {
                return Err(NineDoorBridgeError::Permission);
            }
            let payload = decode_cas_payload(data)?;
            let expected_offset = bundle.manifest_pending.len() as u64;
            let provided_offset = if offset == u64::MAX {
                expected_offset
            } else {
                offset
            };
            if provided_offset != expected_offset {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let new_len = bundle
                .manifest_pending
                .len()
                .saturating_add(payload.len());
            if new_len > CAS_MANIFEST_MAX_BYTES {
                return Err(NineDoorBridgeError::BufferFull);
            }
            bundle.manifest_pending.extend_from_slice(&payload);
            match CasManifest::decode(&bundle.manifest_pending) {
                Ok(manifest) => (
                    Some(manifest),
                    Some(bundle.manifest_pending.clone()),
                ),
                Err(CasManifestError::UnexpectedEof) => return Ok(data.len() as u32),
                Err(_) => {
                    bundle.manifest_pending.clear();
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
        };
        let Some(manifest) = decoded else {
            return Ok(data.len() as u32);
        };
        if let Err(err) = self.validate_manifest(epoch, &manifest) {
            if let Some(bundle) = self.updates.get_mut(epoch) {
                bundle.manifest_pending.clear();
            }
            return Err(err);
        }
        if let Some(bundle) = self.updates.get_mut(epoch) {
            bundle.manifest_bytes = manifest_bytes;
            bundle.manifest_pending.clear();
            bundle.manifest = Some(manifest);
        }
        Ok(data.len() as u32)
    }

    fn append_chunk(
        &mut self,
        epoch: &str,
        digest: &[u8; 32],
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorBridgeError> {
        self.ensure_enabled()?;
        self.ensure_update(epoch)?;
        self.append_chunk_internal(epoch, digest, offset, data)
    }

    fn append_chunk_internal(
        &mut self,
        label: &str,
        digest: &[u8; 32],
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorBridgeError> {
        let payload = decode_cas_payload(data)?;
        if let Some(existing) = self.chunks.get(digest) {
            if offset != 0 && offset != u64::MAX {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if existing.as_slice() == payload.as_slice() {
                return Ok(data.len() as u32);
            }
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let chunk_bytes = self.chunk_bytes();
        if payload.len() > chunk_bytes {
            return Err(NineDoorBridgeError::BufferFull);
        }
        if !self.can_reserve_bytes(payload.len()) {
            return Err(NineDoorBridgeError::BufferFull);
        }
        let mut quarantine = None;
        {
            let pending = self.pending_chunks.entry(*digest).or_default();
            let expected_offset = pending.len() as u64;
            let provided_offset = if offset == u64::MAX {
                expected_offset
            } else {
                offset
            };
            if provided_offset != expected_offset {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            pending.extend_from_slice(&payload);
            self.bytes_used = self.bytes_used.saturating_add(payload.len());
            let pending_len = pending.len();
            if pending_len < chunk_bytes {
                return Ok(data.len() as u32);
            }
            if pending_len > chunk_bytes {
                pending.clear();
                self.bytes_used = self.bytes_used.saturating_sub(pending_len);
                return Err(NineDoorBridgeError::BufferFull);
            }
            let actual = Sha256::digest(pending.as_slice());
            if actual.as_slice() != digest {
                let mut actual_bytes = [0u8; 32];
                actual_bytes.copy_from_slice(actual.as_slice());
                pending.clear();
                quarantine = Some((actual_bytes, pending_len));
            }
        }
        if let Some((actual_bytes, pending_len)) = quarantine {
            self.quarantine_chunk(label, digest, &actual_bytes, pending_len);
            self.bytes_used = self.bytes_used.saturating_sub(pending_len);
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let committed = self.pending_chunks.remove(digest).unwrap_or_default();
        self.chunks.insert(*digest, committed);
        Ok(data.len() as u32)
    }

    fn append_model_file(
        &mut self,
        digest: &[u8; 32],
        kind: ModelFileKind,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorBridgeError> {
        self.ensure_models_enabled()?;
        self.ensure_model_entry(digest)?;
        match kind {
            ModelFileKind::Weights => {
                if self
                    .models
                    .get(digest)
                    .is_some_and(|model| model.weights_committed)
                {
                    return Err(NineDoorBridgeError::Permission);
                }
                let count = self.append_chunk_internal("model", digest, offset, data)?;
                if self.chunks.contains_key(digest) {
                    if let Some(model) = self.models.get_mut(digest) {
                        model.weights_committed = true;
                    }
                }
                Ok(count)
            }
            ModelFileKind::Schema => {
                if self
                    .models
                    .get(digest)
                    .and_then(|model| model.schema.as_ref())
                    .is_some()
                {
                    return Err(NineDoorBridgeError::Permission);
                }
                let payload = decode_cas_payload(data)?;
                let chunk_bytes = self.chunk_bytes();
                if payload.len() > chunk_bytes {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                let expected_offset = self
                    .models
                    .get(digest)
                    .and_then(|model| model.schema.as_ref())
                    .map_or(0, |data| data.len()) as u64;
                let provided_offset = if offset == u64::MAX {
                    expected_offset
                } else {
                    offset
                };
                if provided_offset != expected_offset {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if !self.can_reserve_bytes(payload.len()) {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                {
                    let model = self.models.get_mut(digest).expect("model must exist");
                    model
                        .schema
                        .get_or_insert_with(Vec::new)
                        .extend_from_slice(&payload);
                }
                self.bytes_used = self.bytes_used.saturating_add(payload.len());
                Ok(data.len() as u32)
            }
            ModelFileKind::Signature => {
                if self
                    .models
                    .get(digest)
                    .and_then(|model| model.signature.as_ref())
                    .is_some()
                {
                    return Err(NineDoorBridgeError::Permission);
                }
                let payload = decode_cas_payload(data)?;
                let chunk_bytes = self.chunk_bytes();
                if payload.len() > chunk_bytes {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                let expected_offset = self
                    .models
                    .get(digest)
                    .and_then(|model| model.signature.as_ref())
                    .map_or(0, |data| data.len()) as u64;
                let provided_offset = if offset == u64::MAX {
                    expected_offset
                } else {
                    offset
                };
                if provided_offset != expected_offset {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if !self.can_reserve_bytes(payload.len()) {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                {
                    let model = self.models.get_mut(digest).expect("model must exist");
                    model
                        .signature
                        .get_or_insert_with(Vec::new)
                        .extend_from_slice(&payload);
                }
                self.bytes_used = self.bytes_used.saturating_add(payload.len());
                Ok(data.len() as u32)
            }
        }
    }

    fn validate_manifest(
        &mut self,
        epoch: &str,
        manifest: &CasManifest,
    ) -> Result<(), NineDoorBridgeError> {
        if manifest.schema != CAS_MANIFEST_SCHEMA {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if manifest.epoch != epoch {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if manifest.chunk_bytes as usize != self.chunk_bytes() {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let expected_bytes = (manifest.chunks.len() as u64)
            .saturating_mul(manifest.chunk_bytes as u64);
        if manifest.payload_bytes != expected_bytes {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if manifest.chunks.len() > CAS_MAX_CHUNKS {
            return Err(NineDoorBridgeError::BufferFull);
        }
        if let Some(delta) = &manifest.delta {
            if !self.config.delta_enable {
                return Err(NineDoorBridgeError::Permission);
            }
            let base = self
                .updates
                .get(&delta.base_epoch)
                .and_then(|bundle| bundle.manifest.as_ref())
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            if base.delta.is_some() {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if base.payload_sha256 != delta.base_sha256 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
        }
        if self.config.signing_required && manifest.signature.is_none() {
            self.log_event(&format!(
                "cas-manifest rejected epoch={} reason=missing-signature",
                epoch
            ));
            return Err(NineDoorBridgeError::Permission);
        }
        if let Some(signature) = manifest.signature {
            let key = self.config.signing_key.ok_or_else(|| {
                self.log_event(&format!(
                    "cas-manifest rejected epoch={} reason=signing-key-missing",
                    epoch
                ));
                NineDoorBridgeError::Permission
            })?;
            let verifying_key = SigningKey::from_bytes(&key).verifying_key();
            let payload = manifest
                .signature_payload()
                .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            let signature = Signature::from_bytes(&signature);
            if verifying_key.verify(&payload, &signature).is_err() {
                self.log_event(&format!(
                    "cas-manifest rejected epoch={} reason=signature-failed",
                    epoch
                ));
                return Err(NineDoorBridgeError::Permission);
            }
        }
        let payload = self.assemble_payload(manifest)?;
        let computed = Sha256::digest(&payload);
        if computed.as_slice() != manifest.payload_sha256 {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let delta_label = if manifest.delta.is_some() { "delta" } else { "base" };
        let payload_hex = hex::encode(manifest.payload_sha256);
        self.log_event(&format!(
            "cas-manifest accepted epoch={} kind={} payload_sha256={payload_hex} chunks={}",
            epoch,
            delta_label,
            manifest.chunks.len()
        ));
        Ok(())
    }

    fn assemble_payload(&self, manifest: &CasManifest) -> Result<Vec<u8>, NineDoorBridgeError> {
        let mut payload = Vec::new();
        if let Some(delta) = &manifest.delta {
            let base = self
                .updates
                .get(&delta.base_epoch)
                .and_then(|bundle| bundle.manifest.as_ref())
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            for digest in &base.chunks {
                let chunk = self.chunks.get(digest).ok_or(NineDoorBridgeError::InvalidPath)?;
                payload.extend_from_slice(chunk);
            }
        }
        for digest in &manifest.chunks {
            let chunk = self.chunks.get(digest).ok_or(NineDoorBridgeError::InvalidPath)?;
            payload.extend_from_slice(chunk);
        }
        Ok(payload)
    }

    fn list_updates(&self) -> Vec<String> {
        self.updates.keys().cloned().collect()
    }

    fn list_models(&self) -> Vec<String> {
        self.models.keys().map(hex::encode).collect()
    }

    fn list_update_chunks(&self, epoch: &str) -> Vec<String> {
        let Some(manifest) = self
            .updates
            .get(epoch)
            .and_then(|bundle| bundle.manifest.as_ref())
        else {
            return Vec::new();
        };
        let mut entries: Vec<String> = manifest.chunks.iter().map(hex::encode).collect();
        entries.sort();
        entries
    }

    fn list_model_entries(&self, digest: &[u8; 32]) -> Vec<String> {
        let Some(model) = self.models.get(digest) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        entries.push("weights".to_owned());
        if model.schema.is_some() {
            entries.push("schema".to_owned());
        }
        if model.signature.is_some() {
            entries.push("signature".to_owned());
        }
        entries.sort();
        entries
    }

    fn ensure_enabled(&self) -> Result<(), NineDoorBridgeError> {
        if self.config.enable {
            Ok(())
        } else {
            Err(NineDoorBridgeError::InvalidPath)
        }
    }

    fn ensure_models_enabled(&self) -> Result<(), NineDoorBridgeError> {
        if self.config.enable && self.config.models_enabled {
            Ok(())
        } else {
            Err(NineDoorBridgeError::InvalidPath)
        }
    }

    fn ensure_update(&mut self, epoch: &str) -> Result<(), NineDoorBridgeError> {
        self.ensure_enabled()?;
        validate_epoch(epoch)?;
        if self.updates.contains_key(epoch) {
            return Ok(());
        }
        if self.updates.len() >= CAS_MAX_UPDATES {
            return Err(NineDoorBridgeError::BufferFull);
        }
        self.updates
            .insert(epoch.to_owned(), UpdateBundle::default());
        Ok(())
    }

    fn ensure_model_entry(&mut self, digest: &[u8; 32]) -> Result<(), NineDoorBridgeError> {
        self.ensure_models_enabled()?;
        if self.models.contains_key(digest) {
            return Ok(());
        }
        if self.models.len() >= CAS_MAX_MODELS {
            return Err(NineDoorBridgeError::BufferFull);
        }
        self.models.insert(*digest, ModelBundle::default());
        Ok(())
    }

    fn can_reserve_bytes(&self, additional: usize) -> bool {
        if self.chunk_bytes() == 0 {
            return false;
        }
        let max_bytes = self.chunk_bytes().saturating_mul(CAS_MAX_CHUNKS);
        self.bytes_used.saturating_add(additional) <= max_bytes
    }

    fn chunk_bytes(&self) -> usize {
        self.config.chunk_bytes as usize
    }

    fn quarantine_chunk(&mut self, epoch: &str, expected: &[u8; 32], actual: &[u8], bytes: usize) {
        let entry = QuarantineEntry {
            epoch: epoch.to_owned(),
            expected: hex::encode(expected),
            actual: hex::encode(actual),
            bytes,
        };
        if self.quarantine.len() >= CAS_QUARANTINE_LIMIT {
            let _ = self.quarantine.pop_front();
        }
        self.log_event(&format!(
            "cas-chunk quarantined epoch={} expected={} actual={} bytes={}",
            entry.epoch, entry.expected, entry.actual, entry.bytes
        ));
        self.quarantine.push_back(entry);
    }

    fn log_event(&self, message: &str) {
        log_buffer::append_log_line(message);
    }
}

#[derive(Debug, Default)]
struct UpdateBundle {
    manifest_bytes: Option<Vec<u8>>,
    manifest_pending: Vec<u8>,
    manifest: Option<CasManifest>,
}

#[derive(Debug)]
struct UpdateStatusPayloads {
    text: Vec<u8>,
    cbor: Vec<u8>,
}

#[derive(Debug)]
struct UpdateStatusSnapshot {
    epoch: String,
    state: &'static str,
    manifest_bytes: usize,
    manifest_pending_bytes: usize,
    chunks_expected: usize,
    chunks_committed: usize,
    chunks_pending: usize,
    chunks_missing: usize,
    payload_bytes: u64,
    payload_sha256: Option<[u8; 32]>,
    delta_base_epoch: Option<String>,
    delta_base_sha256: Option<[u8; 32]>,
}

#[derive(Debug, Default)]
struct ModelBundle {
    weights_committed: bool,
    schema: Option<Vec<u8>>,
    signature: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct BindEntry {
    from: String,
    to: String,
}

#[derive(Debug)]
struct QuarantineEntry {
    epoch: String,
    expected: String,
    actual: String,
    bytes: usize,
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
enum SidecarKind {
    Bus,
    Lora,
}

impl SidecarKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bus => "bus",
            Self::Lora => "lora",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SidecarAccess {
    List,
    Read,
    Write,
}

#[derive(Debug)]
struct SidecarState {
    bus: SidecarBusState,
    lora: SidecarLoraState,
}

impl SidecarState {
    fn new() -> Self {
        let config = generated::sidecar_config();
        let bus = SidecarBusState::new(config.modbus, config.dnp3);
        let lora = SidecarLoraState::new(config.lora);
        Self { bus, lora }
    }

    fn push_root_entries(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let mut seen: Vec<String> = Vec::new();
        self.bus.push_root_entries(output, &mut seen)?;
        self.lora.push_root_entries(output, &mut seen)?;
        Ok(())
    }

    fn kind_for_path(&self, path: &[&str]) -> Option<SidecarKind> {
        if self.bus.matches_path(path) {
            return Some(SidecarKind::Bus);
        }
        if self.lora.matches_path(path) {
            return Some(SidecarKind::Lora);
        }
        None
    }

    fn allowed_prefix(&self, kind: SidecarKind, scope: Option<&str>, path: &[&str]) -> bool {
        match kind {
            SidecarKind::Bus => self.bus.allowed_prefix(scope, path),
            SidecarKind::Lora => self.lora.allowed_prefix(scope, path),
        }
    }

    fn allowed_path(&self, kind: SidecarKind, scope: Option<&str>, path: &[&str]) -> bool {
        match kind {
            SidecarKind::Bus => self.bus.allowed_path(scope, path),
            SidecarKind::Lora => self.lora.allowed_path(scope, path),
        }
    }

    fn list(
        &self,
        path: &[&str],
    ) -> Option<
        Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>,
    > {
        self.bus.list(path).or_else(|| self.lora.list(path))
    }

    fn read(&self, path: &[&str]) -> Option<Vec<u8>> {
        self.bus.read(path).or_else(|| self.lora.read(path))
    }

    fn write(
        &mut self,
        path: &[&str],
        payload: &[u8],
    ) -> Result<Option<u32>, NineDoorBridgeError> {
        if let Some(count) = self.bus.write(path, payload, SIDECAR_LOG_MAX_BYTES)? {
            return Ok(Some(count));
        }
        if let Some(count) = self.lora.write(path, payload, SIDECAR_LOG_MAX_BYTES)? {
            return Ok(Some(count));
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarBusFile {
    Ctl,
    Telemetry,
    Link,
    Replay,
    Spool,
}

#[derive(Debug)]
struct SidecarBusAdapterState {
    mount_root: Vec<String>,
    mount_label: String,
    scope: String,
    spool: OfflineSpool,
    link_state: LinkState,
    telemetry: Vec<u8>,
    ctl: Vec<u8>,
    link: Vec<u8>,
    replay: Vec<u8>,
}

impl SidecarBusAdapterState {
    fn match_file(&self, path: &[&str]) -> Option<SidecarBusFile> {
        if path.len() != self.mount_root.len().saturating_add(1) {
            return None;
        }
        if !segments_start_with(path, &self.mount_root) {
            return None;
        }
        match path.last()? {
            &"ctl" => Some(SidecarBusFile::Ctl),
            &"telemetry" => Some(SidecarBusFile::Telemetry),
            &"link" => Some(SidecarBusFile::Link),
            &"replay" => Some(SidecarBusFile::Replay),
            &"spool" => Some(SidecarBusFile::Spool),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct SidecarBusState {
    adapters: Vec<SidecarBusAdapterState>,
}

impl SidecarBusState {
    fn new(modbus: generated::SidecarBusConfig, dnp3: generated::SidecarBusConfig) -> Self {
        let mut adapters = Vec::new();
        Self::push_adapters(&mut adapters, modbus);
        Self::push_adapters(&mut adapters, dnp3);
        Self { adapters }
    }

    fn push_adapters(adapters: &mut Vec<SidecarBusAdapterState>, config: generated::SidecarBusConfig) {
        if !config.enable {
            return;
        }
        for adapter in config.adapters.iter().copied() {
            let mount_root = sidecar_mount_root(config.mount_at, adapter.mount);
            let spool = SpoolConfig::new(
                adapter.spool.max_entries as usize,
                adapter.spool.max_bytes as usize,
            );
            adapters.push(SidecarBusAdapterState {
                mount_root,
                mount_label: adapter.mount.to_owned(),
                scope: adapter.scope.to_owned(),
                spool: OfflineSpool::new(spool),
                link_state: LinkState::Offline,
                telemetry: Vec::new(),
                ctl: Vec::new(),
                link: Vec::new(),
                replay: Vec::new(),
            });
        }
    }

    fn push_root_entries(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        seen: &mut Vec<String>,
    ) -> Result<(), NineDoorBridgeError> {
        for adapter in &self.adapters {
            if let Some(label) = adapter.mount_root.first() {
                if !seen.iter().any(|entry| entry == label) {
                    push_list_entry(output, label.as_str())?;
                    seen.push(label.clone());
                }
            }
        }
        Ok(())
    }

    fn matches_path(&self, path: &[&str]) -> bool {
        self.adapters
            .iter()
            .any(|adapter| segments_match_prefix(path, &adapter.mount_root))
    }

    fn allowed_prefix(&self, scope: Option<&str>, path: &[&str]) -> bool {
        let Some(scope) = scope else {
            return false;
        };
        self.adapters.iter().any(|adapter| {
            adapter.scope == scope && segments_match_prefix(path, &adapter.mount_root)
        })
    }

    fn allowed_path(&self, scope: Option<&str>, path: &[&str]) -> bool {
        let Some(scope) = scope else {
            return false;
        };
        self.adapters
            .iter()
            .any(|adapter| adapter.scope == scope && segments_start_with(path, &adapter.mount_root))
    }

    fn list(
        &self,
        path: &[&str],
    ) -> Option<
        Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>,
    > {
        if self.adapters.is_empty() {
            return None;
        }
        let mut output = HeaplessVec::new();
        let mut matched_root = false;
        for adapter in &self.adapters {
            let root_len = adapter.mount_root.len().saturating_sub(1);
            let root = &adapter.mount_root[..root_len];
            if segments_equal(path, root) {
                matched_root = true;
                if push_list_entry(&mut output, adapter.mount_label.as_str()).is_err() {
                    return Some(Err(NineDoorBridgeError::BufferFull));
                }
            }
        }
        if matched_root {
            return Some(Ok(output));
        }
        for adapter in &self.adapters {
            if segments_equal(path, &adapter.mount_root) {
                return Some(list_from_slice(&["ctl", "telemetry", "link", "replay", "spool"]));
            }
        }
        None
    }

    fn read(&self, path: &[&str]) -> Option<Vec<u8>> {
        let (adapter, file) = self.adapter_for_path(path)?;
        match file {
            SidecarBusFile::Ctl => Some(adapter.ctl.clone()),
            SidecarBusFile::Telemetry => Some(adapter.telemetry.clone()),
            SidecarBusFile::Link => Some(adapter.link.clone()),
            SidecarBusFile::Replay => Some(adapter.replay.clone()),
            SidecarBusFile::Spool => Some(render_spool_status(&adapter.spool, SIDECAR_LOG_MAX_BYTES)),
        }
    }

    fn write(
        &mut self,
        path: &[&str],
        data: &[u8],
        max_bytes: usize,
    ) -> Result<Option<u32>, NineDoorBridgeError> {
        let Some((adapter, file)) = self.adapter_for_path_mut(path) else {
            return Ok(None);
        };
        match file {
            SidecarBusFile::Ctl => Ok(Some(append_sidecar_bounded(
                &mut adapter.ctl,
                data,
                max_bytes,
            )?)),
            SidecarBusFile::Link => {
                let text = core::str::from_utf8(trim_payload(data))
                    .map_err(|_| NineDoorBridgeError::InvalidPayload)?
                    .trim();
                match text {
                    "online" => adapter.link_state = LinkState::Online,
                    "offline" => adapter.link_state = LinkState::Offline,
                    _ => return Err(NineDoorBridgeError::InvalidPayload),
                }
                Ok(Some(append_sidecar_bounded(
                    &mut adapter.link,
                    data,
                    max_bytes,
                )?))
            }
            SidecarBusFile::Telemetry => match adapter.link_state {
                LinkState::Online => Ok(Some(append_sidecar_bounded(
                    &mut adapter.telemetry,
                    data,
                    max_bytes,
                )?)),
                LinkState::Offline => {
                    let payload = ensure_line_terminated(data);
                    match adapter.spool.push(&payload) {
                        Ok(_) => Ok(Some(payload.len() as u32)),
                        Err(SpoolError::Full | SpoolError::Oversize { .. }) => {
                            Err(NineDoorBridgeError::InvalidPayload)
                        }
                    }
                }
            },
            SidecarBusFile::Replay => {
                let snapshot = adapter.spool.snapshot();
                let total_bytes: usize = snapshot.iter().map(|frame| frame.payload.len()).sum();
                if adapter.telemetry.len().saturating_add(total_bytes) > max_bytes {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                let drained = adapter.spool.drain();
                for frame in drained {
                    adapter.telemetry.extend_from_slice(&frame.payload);
                }
                let summary = format!("replay entries={} bytes={}\n", snapshot.len(), total_bytes);
                Ok(Some(append_sidecar_bounded(
                    &mut adapter.replay,
                    summary.as_bytes(),
                    max_bytes,
                )?))
            }
            SidecarBusFile::Spool => Err(NineDoorBridgeError::Permission),
        }
    }

    fn adapter_for_path(&self, path: &[&str]) -> Option<(&SidecarBusAdapterState, SidecarBusFile)> {
        self.adapters
            .iter()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }

    fn adapter_for_path_mut(
        &mut self,
        path: &[&str],
    ) -> Option<(&mut SidecarBusAdapterState, SidecarBusFile)> {
        self.adapters
            .iter_mut()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarLoraFile {
    Ctl,
    Telemetry,
    Tamper,
}

#[derive(Debug)]
struct SidecarLoraAdapterState {
    mount_root: Vec<String>,
    mount_label: String,
    scope: String,
    guard: DutyCycleGuard,
    tamper: TamperLog,
    telemetry: Vec<u8>,
    ctl: Vec<u8>,
}

impl SidecarLoraAdapterState {
    fn match_file(&self, path: &[&str]) -> Option<SidecarLoraFile> {
        if path.len() != self.mount_root.len().saturating_add(1) {
            return None;
        }
        if !segments_start_with(path, &self.mount_root) {
            return None;
        }
        match path.last()? {
            &"ctl" => Some(SidecarLoraFile::Ctl),
            &"telemetry" => Some(SidecarLoraFile::Telemetry),
            &"tamper" => Some(SidecarLoraFile::Tamper),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct SidecarLoraState {
    adapters: Vec<SidecarLoraAdapterState>,
    clock_ms: u64,
}

impl SidecarLoraState {
    fn new(config: generated::SidecarLoraConfig) -> Self {
        if !config.enable {
            return Self {
                adapters: Vec::new(),
                clock_ms: 0,
            };
        }
        let mut adapters = Vec::new();
        for adapter in config.adapters.iter().copied() {
            let mount_root = sidecar_mount_root(config.mount_at, adapter.mount);
            let duty_cycle = DutyCycleConfig {
                duty_cycle_percent: adapter.duty_cycle_percent,
                window_ms: adapter.window_ms,
                max_payload_bytes: adapter.max_payload_bytes,
            };
            adapters.push(SidecarLoraAdapterState {
                mount_root,
                mount_label: adapter.mount.to_owned(),
                scope: adapter.scope.to_owned(),
                guard: DutyCycleGuard::new(duty_cycle),
                tamper: TamperLog::new(adapter.tamper_log_max_entries as usize),
                telemetry: Vec::new(),
                ctl: Vec::new(),
            });
        }
        Self { adapters, clock_ms: 0 }
    }

    fn push_root_entries(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        seen: &mut Vec<String>,
    ) -> Result<(), NineDoorBridgeError> {
        for adapter in &self.adapters {
            if let Some(label) = adapter.mount_root.first() {
                if !seen.iter().any(|entry| entry == label) {
                    push_list_entry(output, label.as_str())?;
                    seen.push(label.clone());
                }
            }
        }
        Ok(())
    }

    fn matches_path(&self, path: &[&str]) -> bool {
        self.adapters
            .iter()
            .any(|adapter| segments_match_prefix(path, &adapter.mount_root))
    }

    fn allowed_prefix(&self, scope: Option<&str>, path: &[&str]) -> bool {
        let Some(scope) = scope else {
            return false;
        };
        self.adapters.iter().any(|adapter| {
            adapter.scope == scope && segments_match_prefix(path, &adapter.mount_root)
        })
    }

    fn allowed_path(&self, scope: Option<&str>, path: &[&str]) -> bool {
        let Some(scope) = scope else {
            return false;
        };
        self.adapters
            .iter()
            .any(|adapter| adapter.scope == scope && segments_start_with(path, &adapter.mount_root))
    }

    fn list(
        &self,
        path: &[&str],
    ) -> Option<
        Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>,
    > {
        if self.adapters.is_empty() {
            return None;
        }
        let mut output = HeaplessVec::new();
        let mut matched_root = false;
        for adapter in &self.adapters {
            let root_len = adapter.mount_root.len().saturating_sub(1);
            let root = &adapter.mount_root[..root_len];
            if segments_equal(path, root) {
                matched_root = true;
                if push_list_entry(&mut output, adapter.mount_label.as_str()).is_err() {
                    return Some(Err(NineDoorBridgeError::BufferFull));
                }
            }
        }
        if matched_root {
            return Some(Ok(output));
        }
        for adapter in &self.adapters {
            if segments_equal(path, &adapter.mount_root) {
                return Some(list_from_slice(&["ctl", "telemetry", "tamper"]));
            }
        }
        None
    }

    fn read(&self, path: &[&str]) -> Option<Vec<u8>> {
        let (adapter, file) = self.adapter_for_path(path)?;
        match file {
            SidecarLoraFile::Ctl => Some(adapter.ctl.clone()),
            SidecarLoraFile::Telemetry => Some(adapter.telemetry.clone()),
            SidecarLoraFile::Tamper => Some(render_tamper_log(
                adapter.tamper.snapshot(),
                SIDECAR_LOG_MAX_BYTES,
            )),
        }
    }

    fn write(
        &mut self,
        path: &[&str],
        data: &[u8],
        max_bytes: usize,
    ) -> Result<Option<u32>, NineDoorBridgeError> {
        let Some((index, file)) = self.adapter_index_for_path(path) else {
            return Ok(None);
        };
        match file {
            SidecarLoraFile::Ctl => {
                let count = {
                    let adapter = &mut self.adapters[index];
                    append_sidecar_bounded(&mut adapter.ctl, data, max_bytes)?
                };
                let now_ms = self.next_clock();
                let adapter = &mut self.adapters[index];
                match adapter.guard.attempt(now_ms, data.len() as u32) {
                    Ok(()) => {
                        append_sidecar_bounded(&mut adapter.telemetry, data, max_bytes)?;
                        Ok(Some(count))
                    }
                    Err(reason) => {
                        adapter.tamper.push(TamperEntry {
                            timestamp_ms: now_ms,
                            reason,
                            payload_bytes: data.len() as u32,
                        });
                        Err(NineDoorBridgeError::InvalidPayload)
                    }
                }
            }
            SidecarLoraFile::Telemetry | SidecarLoraFile::Tamper => Err(NineDoorBridgeError::Permission),
        }
    }

    fn adapter_for_path(&self, path: &[&str]) -> Option<(&SidecarLoraAdapterState, SidecarLoraFile)> {
        self.adapters
            .iter()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }

    fn adapter_index_for_path(&self, path: &[&str]) -> Option<(usize, SidecarLoraFile)> {
        self.adapters
            .iter()
            .enumerate()
            .find_map(|(idx, adapter)| adapter.match_file(path).map(|file| (idx, file)))
    }

    fn adapter_for_path_mut(
        &mut self,
        path: &[&str],
    ) -> Option<(&mut SidecarLoraAdapterState, SidecarLoraFile)> {
        self.adapters
            .iter_mut()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }

    fn next_clock(&mut self) -> u64 {
        self.clock_ms = self.clock_ms.saturating_add(1);
        self.clock_ms
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

    fn preflight_req_text(&self) -> Result<Vec<u8>, NineDoorBridgeError> {
        let mut total = 0usize;
        let mut queued = 0usize;
        let mut consumed = 0usize;
        for action in &self.actions {
            total = total.saturating_add(1);
            if action.consumed {
                consumed = consumed.saturating_add(1);
            } else {
                queued = queued.saturating_add(1);
            }
        }
        let mut text = String::new();
        let _ = writeln!(text, "req total={} queued={} consumed={}", total, queued, consumed);
        for action in &self.actions {
            let state = if action.consumed { "consumed" } else { "queued" };
            let _ = writeln!(
                text,
                "req id={} target={} decision={} state={}",
                action.id,
                action.target,
                action.decision.as_str(),
                state
            );
        }
        ensure_ui_stream_len(text.len())?;
        Ok(text.into_bytes())
    }

    fn preflight_req_cbor(&self) -> Result<Vec<u8>, NineDoorBridgeError> {
        let mut total = 0usize;
        let mut queued = 0usize;
        let mut consumed = 0usize;
        for action in &self.actions {
            total = total.saturating_add(1);
            if action.consumed {
                consumed = consumed.saturating_add(1);
            } else {
                queued = queued.saturating_add(1);
            }
        }
        let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
        writer.map(4).map_err(cbor_error)?;
        writer.text("total").and_then(|_| writer.unsigned(total as u64)).map_err(cbor_error)?;
        writer.text("queued").and_then(|_| writer.unsigned(queued as u64)).map_err(cbor_error)?;
        writer.text("consumed").and_then(|_| writer.unsigned(consumed as u64)).map_err(cbor_error)?;
        writer.text("actions").and_then(|_| writer.array(self.actions.len())).map_err(cbor_error)?;
        for action in &self.actions {
            let state = if action.consumed { "consumed" } else { "queued" };
            writer.map(4)
                .and_then(|_| writer.text("id"))
                .and_then(|_| writer.text(&action.id))
                .and_then(|_| writer.text("target"))
                .and_then(|_| writer.text(&action.target))
                .and_then(|_| writer.text("decision"))
                .and_then(|_| writer.text(action.decision.as_str()))
                .and_then(|_| writer.text("state"))
                .and_then(|_| writer.text(state))
                .map_err(cbor_error)?;
        }
        Ok(writer.finish())
    }

    fn preflight_diff_text(&self) -> Result<Vec<u8>, NineDoorBridgeError> {
        let mut unmatched = 0usize;
        for action in &self.actions {
            if !self
                .rules
                .iter()
                .any(|rule| path_matches_pattern(rule.target, action.target.as_str()))
            {
                unmatched = unmatched.saturating_add(1);
            }
        }
        let mut text = String::new();
        let _ = writeln!(
            text,
            "diff rules={} actions={} unmatched={}",
            self.rules.len(),
            self.actions.len(),
            unmatched
        );
        for rule in self.rules.iter() {
            let mut queued = 0usize;
            let mut consumed = 0usize;
            for action in &self.actions {
                if path_matches_pattern(rule.target, action.target.as_str()) {
                    if action.consumed {
                        consumed = consumed.saturating_add(1);
                    } else {
                        queued = queued.saturating_add(1);
                    }
                }
            }
            let _ = writeln!(
                text,
                "rule id={} target={} queued={} consumed={}",
                rule.id, rule.target, queued, consumed
            );
        }
        ensure_ui_stream_len(text.len())?;
        Ok(text.into_bytes())
    }

    fn preflight_diff_cbor(&self) -> Result<Vec<u8>, NineDoorBridgeError> {
        let mut unmatched = 0usize;
        for action in &self.actions {
            if !self
                .rules
                .iter()
                .any(|rule| path_matches_pattern(rule.target, action.target.as_str()))
            {
                unmatched = unmatched.saturating_add(1);
            }
        }
        let mut rule_counts = Vec::with_capacity(self.rules.len());
        for rule in self.rules.iter() {
            let mut queued = 0usize;
            let mut consumed = 0usize;
            for action in &self.actions {
                if path_matches_pattern(rule.target, action.target.as_str()) {
                    if action.consumed {
                        consumed = consumed.saturating_add(1);
                    } else {
                        queued = queued.saturating_add(1);
                    }
                }
            }
            rule_counts.push((queued, consumed));
        }
        let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
        writer.map(4).map_err(cbor_error)?;
        writer.text("rules").and_then(|_| writer.unsigned(self.rules.len() as u64)).map_err(cbor_error)?;
        writer.text("actions").and_then(|_| writer.unsigned(self.actions.len() as u64)).map_err(cbor_error)?;
        writer.text("unmatched").and_then(|_| writer.unsigned(unmatched as u64)).map_err(cbor_error)?;
        writer.text("entries").and_then(|_| writer.array(self.rules.len())).map_err(cbor_error)?;
        for (rule, (queued, consumed)) in self.rules.iter().zip(rule_counts.iter()) {
            writer.map(4)
                .and_then(|_| writer.text("id"))
                .and_then(|_| writer.text(rule.id))
                .and_then(|_| writer.text("target"))
                .and_then(|_| writer.text(rule.target))
                .and_then(|_| writer.text("queued"))
                .and_then(|_| writer.unsigned(*queued as u64))
                .and_then(|_| writer.text("consumed"))
                .and_then(|_| writer.unsigned(*consumed as u64))
                .map_err(cbor_error)?;
        }
        Ok(writer.finish())
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
struct AuditState {
    enabled: bool,
    limits: AuditLimits,
    replay_enabled: bool,
    replay_max_entries: usize,
    journal: BoundedLog,
    decisions: BoundedLog,
    replay_entries: VecDeque<ReplayEntry>,
    sequence: u64,
    export_snapshot: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct AuditLimits {
    journal_max_bytes: usize,
    decisions_max_bytes: usize,
}

impl AuditState {
    fn new(config: generated::AuditConfig) -> Self {
        let limits = AuditLimits {
            journal_max_bytes: config.journal_max_bytes as usize,
            decisions_max_bytes: config.decisions_max_bytes as usize,
        };
        let journal = BoundedLog::new(limits.journal_max_bytes);
        let decisions = BoundedLog::new(limits.decisions_max_bytes);
        let replay_enabled = config.enable && config.replay_enable;
        let mut state = Self {
            enabled: config.enable,
            limits,
            replay_enabled,
            replay_max_entries: config.replay_max_entries as usize,
            journal,
            decisions,
            replay_entries: VecDeque::new(),
            sequence: 0,
            export_snapshot: Vec::new(),
        };
        state.refresh_export_snapshot();
        state
    }

    fn append_manual_journal(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        validate_json_lines(payload)?;
        let outcome = self.append_journal(payload.as_bytes(), None)?;
        if outcome.dropped_bytes > 0 {
            log_audit_wrap("journal", outcome.dropped_bytes, outcome.new_base);
        }
        Ok(())
    }

    fn record_control(
        &mut self,
        path: &str,
        payload: &str,
        outcome: ControlOutcome,
        role: &str,
        ticket: &str,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.enabled {
            return Ok(());
        }
        let kind = if path == QUEEN_CTL_PATH {
            "queen-ctl"
        } else {
            "host-control"
        };
        let seq = self.next_sequence();
        let path_label = escape_json_string(normalize_path(path).as_str());
        let mut line = String::new();
        let payload = escape_json_string(payload);
        let role = escape_json_string(role);
        let ticket = escape_json_string(ticket);
        write!(
            line,
            "{{\"seq\":{},\"kind\":\"{}\",\"path\":\"{}\",\"payload\":\"{}\",\"outcome\":\"{}\"",
            seq,
            kind,
            path_label,
            payload,
            outcome.status_label()
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        if let Some(error) = outcome.error_detail() {
            let code = escape_json_string(error.code.as_str());
            let message = escape_json_string(error.message);
            write!(
                line,
                ",\"error\":{{\"code\":\"{}\",\"message\":\"{}\"}}",
                code, message
            )
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        write!(
            line,
            ",\"role\":\"{}\",\"ticket\":\"{}\"}}",
            role, ticket
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        let bytes = ensure_line_terminated(line.as_bytes());
        let replay_entry = Some(ReplayEntry::new(bytes.len() as u64, outcome.ack_line()));
        let outcome = self.append_journal_bytes(bytes, replay_entry)?;
        if outcome.dropped_bytes > 0 {
            log_audit_wrap("journal", outcome.dropped_bytes, outcome.new_base);
        }
        Ok(())
    }

    fn record_decision_action(
        &mut self,
        action: &PolicyAction,
        role: &str,
        ticket: &str,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.enabled {
            return Ok(());
        }
        let seq = self.next_sequence();
        let id = escape_json_string(action.id.as_str());
        let target = escape_json_string(action.target.as_str());
        let role = escape_json_string(role);
        let ticket = escape_json_string(ticket);
        let mut line = String::new();
        write!(
            line,
            "{{\"seq\":{},\"kind\":\"policy-action\",\"outcome\":\"{}\",\"id\":\"{}\",\"target\":\"{}\",\"role\":\"{}\",\"ticket\":\"{}\"}}",
            seq,
            action.decision.as_str(),
            id,
            target,
            role,
            ticket
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        let outcome = self.append_decisions(line.as_bytes())?;
        if outcome.dropped_bytes > 0 {
            log_audit_wrap("decisions", outcome.dropped_bytes, outcome.new_base);
        }
        Ok(())
    }

    fn record_decision_gate(
        &mut self,
        path: &str,
        allowance: &PolicyGateAllowance,
        role: &str,
        ticket: &str,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.enabled {
            return Ok(());
        }
        let (id, target) = match allowance {
            PolicyGateAllowance::Action { id, target } => (Some(id.as_str()), Some(target.as_str())),
            PolicyGateAllowance::Ungated | PolicyGateAllowance::NotRequired => return Ok(()),
        };
        let seq = self.next_sequence();
        let path = escape_json_string(normalize_path(path).as_str());
        let role = escape_json_string(role);
        let ticket = escape_json_string(ticket);
        let mut line = String::new();
        write!(
            line,
            "{{\"seq\":{},\"kind\":\"policy-gate\",\"outcome\":\"allow\"",
            seq
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        if let (Some(id), Some(target)) = (id, target) {
            let id = escape_json_string(id);
            let target = escape_json_string(target);
            write!(line, ",\"id\":\"{}\",\"target\":\"{}\"", id, target)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        write!(
            line,
            ",\"path\":\"{}\",\"role\":\"{}\",\"ticket\":\"{}\"}}",
            path, role, ticket
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        let outcome = self.append_decisions(line.as_bytes())?;
        if outcome.dropped_bytes > 0 {
            log_audit_wrap("decisions", outcome.dropped_bytes, outcome.new_base);
        }
        Ok(())
    }

    fn record_decision_gate_denial(
        &mut self,
        path: &str,
        denial: &PolicyGateDenial,
        role: &str,
        ticket: &str,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.enabled {
            return Ok(());
        }
        let seq = self.next_sequence();
        let path = escape_json_string(normalize_path(path).as_str());
        let role = escape_json_string(role);
        let ticket = escape_json_string(ticket);
        let mut line = String::new();
        write!(
            line,
            "{{\"seq\":{},\"kind\":\"policy-gate\",\"outcome\":\"deny\"",
            seq
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        if let PolicyGateDenial::Action { id, target } = denial {
            let id = escape_json_string(id.as_str());
            let target = escape_json_string(target.as_str());
            write!(line, ",\"id\":\"{}\",\"target\":\"{}\"", id, target)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        write!(
            line,
            ",\"path\":\"{}\",\"role\":\"{}\",\"ticket\":\"{}\"}}",
            path, role, ticket
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        let outcome = self.append_decisions(line.as_bytes())?;
        if outcome.dropped_bytes > 0 {
            log_audit_wrap("decisions", outcome.dropped_bytes, outcome.new_base);
        }
        Ok(())
    }

    fn journal_snapshot(&self) -> Vec<u8> {
        self.journal.snapshot()
    }

    fn decisions_snapshot(&self) -> Vec<u8> {
        self.decisions.snapshot()
    }

    fn export_snapshot(&self) -> Vec<u8> {
        self.export_snapshot.clone()
    }

    fn replay_summary(
        &self,
        from: u64,
        max_entries: usize,
    ) -> Result<ReplaySummary, ReplayWindowError> {
        let bounds = self.journal.bounds();
        if from < bounds.base_offset {
            return Err(ReplayWindowError::Stale {
                requested: from,
                available_start: bounds.base_offset,
            });
        }
        if from > bounds.next_offset {
            return Err(ReplayWindowError::Future {
                requested: from,
                available_end: bounds.next_offset,
            });
        }
        let mut sequence = String::new();
        let mut count = 0usize;
        for entry in self.replay_entries.iter() {
            if entry.offset_end <= from {
                continue;
            }
            count = count.saturating_add(1);
            if count > max_entries {
                return Err(ReplayWindowError::TooManyEntries {
                    requested: count,
                    max: max_entries,
                });
            }
            sequence.push_str(entry.ack_line.as_str());
            sequence.push('\n');
        }
        Ok(ReplaySummary {
            from,
            to: bounds.next_offset,
            entries: count,
            sequence,
        })
    }

    fn append_journal(
        &mut self,
        payload: &[u8],
        replay_entry: Option<ReplayEntry>,
    ) -> Result<AuditAppendOutcome, NineDoorBridgeError> {
        let bytes = ensure_line_terminated(payload);
        self.append_journal_bytes(bytes, replay_entry)
    }

    fn append_journal_bytes(
        &mut self,
        bytes: Vec<u8>,
        replay_entry: Option<ReplayEntry>,
    ) -> Result<AuditAppendOutcome, NineDoorBridgeError> {
        let outcome = self.journal.append(bytes)?;
        if let Some(mut replay_entry) = replay_entry {
            replay_entry.offset_start = outcome.offset_start;
            replay_entry.offset_end = outcome.offset_end;
            self.replay_entries.push_back(replay_entry);
        }
        self.trim_replay_entries();
        self.refresh_export_snapshot();
        Ok(outcome)
    }

    fn append_decisions(&mut self, payload: &[u8]) -> Result<AuditAppendOutcome, NineDoorBridgeError> {
        let bytes = ensure_line_terminated(payload);
        let outcome = self.decisions.append(bytes)?;
        self.refresh_export_snapshot();
        Ok(outcome)
    }

    fn trim_replay_entries(&mut self) {
        let base = self.journal.bounds().base_offset;
        while let Some(entry) = self.replay_entries.front() {
            if entry.offset_end <= base {
                let _ = self.replay_entries.pop_front();
            } else {
                break;
            }
        }
    }

    fn refresh_export_snapshot(&mut self) {
        let journal_bounds = self.journal.bounds();
        let decisions_bounds = self.decisions.bounds();
        self.export_snapshot = format!(
            "{{\"journal_base\":{},\"journal_next\":{},\"decisions_base\":{},\"decisions_next\":{},\"replay_enabled\":{},\"replay_max_entries\":{}}}\n",
            journal_bounds.base_offset,
            journal_bounds.next_offset,
            decisions_bounds.base_offset,
            decisions_bounds.next_offset,
            self.replay_enabled,
            self.replay_max_entries
        )
        .into_bytes();
    }

    fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        self.sequence
    }
}

#[derive(Debug, Clone)]
struct ControlOutcome {
    status: ControlStatus,
    error: Option<ControlError>,
}

impl ControlOutcome {
    fn ok() -> Self {
        Self {
            status: ControlStatus::Ok,
            error: None,
        }
    }

    fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status: ControlStatus::Err,
            error: Some(ControlError {
                code,
                message: message.into(),
            }),
        }
    }

    fn from_result(result: &Result<(), NineDoorBridgeError>) -> Self {
        match result {
            Ok(()) => Self::ok(),
            Err(err) => Self::from_error(err),
        }
    }

    fn from_error(error: &NineDoorBridgeError) -> Self {
        let code = error_code_for_audit(error);
        ControlOutcome::err(code, format!("{error}"))
    }

    fn status_label(&self) -> &'static str {
        match self.status {
            ControlStatus::Ok => "ok",
            ControlStatus::Err => "err",
        }
    }

    fn error_detail(&self) -> Option<ControlErrorDetail<'_>> {
        self.error.as_ref().map(|err| ControlErrorDetail {
            code: format!("{}", err.code),
            message: err.message.as_str(),
        })
    }

    fn ack_line(&self) -> String {
        match &self.error {
            None => String::from("OK"),
            Some(err) => format!("ERR {} {}", err.code, err.message),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ControlStatus {
    Ok,
    Err,
}

#[derive(Debug, Clone)]
struct ControlError {
    code: ErrorCode,
    message: String,
}

#[derive(Debug)]
struct ControlErrorDetail<'a> {
    code: String,
    message: &'a str,
}

#[derive(Debug)]
struct ReplayState {
    enabled: bool,
    max_entries: usize,
    ctl_max_bytes: u32,
    status_max_bytes: u32,
    ctl_log: Vec<u8>,
    status: Vec<u8>,
}

impl ReplayState {
    fn new(config: generated::AuditConfig) -> Self {
        let enabled = config.enable && config.replay_enable;
        let status = if enabled {
            b"{\"state\":\"idle\"}\n".to_vec()
        } else {
            Vec::new()
        };
        Self {
            enabled,
            max_entries: config.replay_max_entries as usize,
            ctl_max_bytes: config.replay_ctl_max_bytes,
            status_max_bytes: config.replay_status_max_bytes,
            ctl_log: Vec::new(),
            status,
        }
    }

    fn handle_ctl(
        &mut self,
        payload: &str,
        audit: &mut AuditState,
    ) -> Result<(), NineDoorBridgeError> {
        let command = parse_replay_command(payload)?;
        self.append_ctl(payload)?;
        let summary = match audit.replay_summary(command.from, self.max_entries) {
            Ok(summary) => summary,
            Err(err) => {
                let message = err.message();
                self.set_status_err(message.as_str())?;
                return Err(NineDoorBridgeError::InvalidPayload);
            }
        };
        self.set_status_ok(&summary)?;
        Ok(())
    }

    fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    fn status(&self) -> &[u8] {
        &self.status
    }

    fn append_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)
    }

    fn set_status_ok(&mut self, summary: &ReplaySummary) -> Result<(), NineDoorBridgeError> {
        let sequence_hash = format!("{:016x}", fnv1a64(summary.sequence.as_bytes()));
        let payload = format!(
            "{{\"state\":\"ok\",\"from\":{},\"to\":{},\"entries\":{},\"match\":true,\"sequence_fnv1a\":\"{}\"}}\n",
            summary.from,
            summary.to,
            summary.entries,
            sequence_hash
        );
        if payload.len() > self.status_max_bytes as usize {
            return Err(NineDoorBridgeError::BufferFull);
        }
        self.status = payload.into_bytes();
        Ok(())
    }

    fn set_status_err(&mut self, message: &str) -> Result<(), NineDoorBridgeError> {
        let message = escape_json_string(message);
        let payload = format!("{{\"state\":\"err\",\"error\":\"{}\"}}\n", message);
        if payload.len() > self.status_max_bytes as usize {
            return Err(NineDoorBridgeError::BufferFull);
        }
        self.status = payload.into_bytes();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct ReplayCommand {
    from: u64,
}

#[derive(Debug)]
struct ReplaySummary {
    from: u64,
    to: u64,
    entries: usize,
    sequence: String,
}

#[derive(Debug)]
enum ReplayWindowError {
    Stale { requested: u64, available_start: u64 },
    Future { requested: u64, available_end: u64 },
    TooManyEntries { requested: usize, max: usize },
}

impl ReplayWindowError {
    fn message(&self) -> String {
        match self {
            ReplayWindowError::Stale {
                requested,
                available_start,
            } => format!(
                "replay cursor stale requested={} window_start={}",
                requested, available_start
            ),
            ReplayWindowError::Future {
                requested,
                available_end,
            } => format!(
                "replay cursor beyond window requested={} window_end={}",
                requested, available_end
            ),
            ReplayWindowError::TooManyEntries { requested, max } => format!(
                "replay exceeds max entries {} > {}",
                requested, max
            ),
        }
    }
}

#[derive(Debug)]
struct AuditAppendOutcome {
    count: u32,
    dropped_bytes: u64,
    new_base: u64,
    offset_start: u64,
    offset_end: u64,
}

#[derive(Debug, Clone, Copy)]
struct LogBounds {
    base_offset: u64,
    next_offset: u64,
}

#[derive(Debug)]
struct LogEntry {
    bytes: Vec<u8>,
    offset_start: u64,
    offset_end: u64,
}

#[derive(Debug)]
struct ReplayEntry {
    offset_start: u64,
    offset_end: u64,
    ack_line: String,
}

impl ReplayEntry {
    fn new(length: u64, ack_line: String) -> Self {
        Self {
            offset_start: 0,
            offset_end: length,
            ack_line,
        }
    }
}

#[derive(Debug)]
struct BoundedLog {
    entries: VecDeque<LogEntry>,
    capacity: usize,
    total_bytes: usize,
    base_offset: u64,
    next_offset: u64,
}

impl BoundedLog {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            entries: VecDeque::new(),
            capacity,
            total_bytes: 0,
            base_offset: 0,
            next_offset: 0,
        }
    }

    fn bounds(&self) -> LogBounds {
        LogBounds {
            base_offset: self.base_offset,
            next_offset: self.next_offset,
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_bytes);
        for entry in self.entries.iter() {
            out.extend_from_slice(entry.bytes.as_slice());
        }
        out
    }

    fn append(&mut self, bytes: Vec<u8>) -> Result<AuditAppendOutcome, NineDoorBridgeError> {
        if bytes.len() > self.capacity {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let mut dropped_bytes = 0u64;
        while self.total_bytes + bytes.len() > self.capacity {
            if let Some(entry) = self.entries.pop_front() {
                dropped_bytes = dropped_bytes.saturating_add(entry.bytes.len() as u64);
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes.len());
                self.base_offset = entry.offset_end;
            } else {
                break;
            }
        }
        let offset_start = self.next_offset;
        let offset_end = offset_start.saturating_add(bytes.len() as u64);
        self.entries.push_back(LogEntry {
            bytes,
            offset_start,
            offset_end,
        });
        self.total_bytes = self
            .total_bytes
            .saturating_add(self.entries.back().unwrap().bytes.len());
        self.next_offset = offset_end;
        Ok(AuditAppendOutcome {
            count: (offset_end - offset_start) as u32,
            dropped_bytes,
            new_base: self.base_offset,
            offset_start,
            offset_end,
        })
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
    Bind { from: &'a str, to: &'a str },
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

fn sidecar_mount_root(mount_at: &str, adapter_mount: &str) -> Vec<String> {
    let segments = split_path_segments(mount_at);
    let mut root = Vec::new();
    for segment in segments.iter() {
        root.push((*segment).to_owned());
    }
    if !adapter_mount.is_empty() {
        root.push(adapter_mount.to_owned());
    }
    root
}

fn segments_start_with(path: &[&str], prefix: &[String]) -> bool {
    if path.len() < prefix.len() {
        return false;
    }
    path.iter()
        .zip(prefix.iter())
        .all(|(segment, prefix_segment)| *segment == prefix_segment.as_str())
}

fn segments_match_prefix(path: &[&str], prefix: &[String]) -> bool {
    if path.len() >= prefix.len() {
        return segments_start_with(path, prefix);
    }
    prefix
        .iter()
        .zip(path.iter())
        .all(|(prefix_segment, segment)| prefix_segment.as_str() == *segment)
}

fn segments_equal(path: &[&str], other: &[String]) -> bool {
    if path.len() != other.len() {
        return false;
    }
    path.iter()
        .zip(other.iter())
        .all(|(segment, other_segment)| *segment == other_segment.as_str())
}

fn legacy_worker_alias_enabled(sharding: generated::ShardingConfig) -> bool {
    sharding.enabled && sharding.legacy_worker_alias
}

fn worker_shard_label(worker_id: &str, sharding: generated::ShardingConfig) -> String {
    if !sharding.enabled || sharding.shard_bits == 0 {
        return String::from("00");
    }
    let mut hasher = Sha256::new();
    hasher.update(worker_id.as_bytes());
    let digest = hasher.finalize();
    let mut shard = digest[0];
    if sharding.shard_bits < 8 {
        shard >>= 8 - sharding.shard_bits;
    }
    format!("{:02x}", shard)
}

fn shard_label_known(label: &str) -> bool {
    generated::shard_labels().iter().any(|entry| *entry == label)
}

fn parse_shard_worker_root(path: &str) -> Option<(&str, bool)> {
    let segments = split_path_segments(path);
    match segments.as_slice() {
        ["shard", label] => Some((*label, false)),
        ["shard", label, "worker"] => Some((*label, true)),
        _ => None,
    }
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

fn list_shard_labels(
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    for label in generated::shard_labels() {
        push_list_entry(&mut output, label)?;
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

fn cas_lines_from_bytes(
    data: &[u8],
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let encoded = BASE64_STANDARD.encode(data);
    let mut output = HeaplessVec::new();
    let max_payload = (DEFAULT_LINE_CAPACITY.saturating_sub(4) / 4) * 4;
    if encoded.len().saturating_add(4) <= DEFAULT_LINE_CAPACITY {
        let mut line = HeaplessString::new();
        line.push_str("b64:")
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        line.push_str(encoded.as_str())
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        output
            .push(line)
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        return Ok(output);
    }
    for chunk in encoded.as_bytes().chunks(max_payload) {
        let chunk_str =
            core::str::from_utf8(chunk).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        let mut line = HeaplessString::new();
        line.push_str("b64:")
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        line.push_str(chunk_str)
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        output
            .push(line)
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
    }
    Ok(output)
}

fn ensure_ui_stream_len(len: usize) -> Result<(), NineDoorBridgeError> {
    if len > UI_MAX_STREAM_BYTES {
        return Err(NineDoorBridgeError::BufferFull);
    }
    Ok(())
}

fn cbor_error(_: CborError) -> NineDoorBridgeError {
    NineDoorBridgeError::BufferFull
}

fn lines_from_text(
    text: &str,
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    script_lines(text)
}

fn build_update_status_text(
    snapshot: &UpdateStatusSnapshot,
) -> Result<Vec<u8>, NineDoorBridgeError> {
    let payload_sha = snapshot
        .payload_sha256
        .map(hex::encode)
        .unwrap_or_else(|| "none".to_owned());
    let (delta_epoch, delta_sha) = match (&snapshot.delta_base_epoch, snapshot.delta_base_sha256) {
        (Some(epoch), Some(sha)) => (epoch.as_str(), hex::encode(sha)),
        _ => ("none", "none".to_owned()),
    };
    let mut text = String::new();
    let _ = writeln!(
        text,
        "status epoch={} state={}",
        snapshot.epoch,
        snapshot.state
    );
    let _ = writeln!(
        text,
        "manifest_bytes={} manifest_pending_bytes={}",
        snapshot.manifest_bytes,
        snapshot.manifest_pending_bytes
    );
    let _ = writeln!(
        text,
        "chunks_expected={} chunks_committed={} chunks_pending={} chunks_missing={}",
        snapshot.chunks_expected,
        snapshot.chunks_committed,
        snapshot.chunks_pending,
        snapshot.chunks_missing
    );
    let _ = writeln!(
        text,
        "payload_bytes={} payload_sha256={}",
        snapshot.payload_bytes,
        payload_sha
    );
    let _ = writeln!(
        text,
        "delta_base_epoch={} delta_base_sha256={}",
        delta_epoch,
        delta_sha
    );
    ensure_ui_stream_len(text.len())?;
    Ok(text.into_bytes())
}

fn build_update_status_cbor(
    snapshot: &UpdateStatusSnapshot,
) -> Result<Vec<u8>, NineDoorBridgeError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer.map(11).map_err(cbor_error)?;
    writer.text("epoch").and_then(|_| writer.text(snapshot.epoch.as_str())).map_err(cbor_error)?;
    writer.text("state").and_then(|_| writer.text(snapshot.state)).map_err(cbor_error)?;
    writer
        .text("manifest_bytes")
        .and_then(|_| writer.unsigned(snapshot.manifest_bytes as u64))
        .map_err(cbor_error)?;
    writer
        .text("manifest_pending_bytes")
        .and_then(|_| writer.unsigned(snapshot.manifest_pending_bytes as u64))
        .map_err(cbor_error)?;
    writer
        .text("chunks_expected")
        .and_then(|_| writer.unsigned(snapshot.chunks_expected as u64))
        .map_err(cbor_error)?;
    writer
        .text("chunks_committed")
        .and_then(|_| writer.unsigned(snapshot.chunks_committed as u64))
        .map_err(cbor_error)?;
    writer
        .text("chunks_pending")
        .and_then(|_| writer.unsigned(snapshot.chunks_pending as u64))
        .map_err(cbor_error)?;
    writer
        .text("chunks_missing")
        .and_then(|_| writer.unsigned(snapshot.chunks_missing as u64))
        .map_err(cbor_error)?;
    writer
        .text("payload_bytes")
        .and_then(|_| writer.unsigned(snapshot.payload_bytes))
        .map_err(cbor_error)?;
    writer
        .text("payload_sha256")
        .and_then(|_| match snapshot.payload_sha256 {
            Some(sha) => writer.bytes(&sha),
            None => writer.null(),
        })
        .map_err(cbor_error)?;
    writer
        .text("delta")
        .and_then(|_| match (&snapshot.delta_base_epoch, snapshot.delta_base_sha256) {
            (Some(epoch), Some(sha)) => {
                writer.map(2)?;
                writer.text("base_epoch")?;
                writer.text(epoch.as_str())?;
                writer.text("base_sha256")?;
                writer.bytes(&sha)?;
                Ok(())
            }
            _ => writer.null(),
        })
        .map_err(cbor_error)?;
    Ok(writer.finish())
}

fn render_p50_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_P50_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "p50_ms={}", snapshot.p50_ms)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_p95_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_P95_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "p95_ms={}", snapshot.p95_ms)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_backpressure_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_BACKPRESSURE_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "backpressure={}", snapshot.backpressure)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_dropped_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_DROPPED_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "dropped={}", snapshot.dropped)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_queued_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_QUEUED_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "queued={}", snapshot.queued)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

#[derive(Debug, Clone, Copy)]
enum CborError {
    TooLarge,
}

#[derive(Debug)]
struct CborWriter {
    buffer: Vec<u8>,
    max_len: usize,
}

impl CborWriter {
    fn new(max_len: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_len,
        }
    }

    fn finish(self) -> Vec<u8> {
        self.buffer
    }

    fn map(&mut self, len: usize) -> Result<(), CborError> {
        self.write_type_and_len(5, len as u64)
    }

    fn array(&mut self, len: usize) -> Result<(), CborError> {
        self.write_type_and_len(4, len as u64)
    }

    fn text(&mut self, value: &str) -> Result<(), CborError> {
        self.write_type_and_len(3, value.len() as u64)?;
        self.push(value.as_bytes())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), CborError> {
        self.write_type_and_len(2, value.len() as u64)?;
        self.push(value)
    }

    fn unsigned(&mut self, value: u64) -> Result<(), CborError> {
        self.write_type_and_len(0, value)
    }

    fn null(&mut self) -> Result<(), CborError> {
        self.push_u8(0xf6)
    }

    fn write_type_and_len(&mut self, major: u8, len: u64) -> Result<(), CborError> {
        let (info, extra) = if len <= 23 {
            (len as u8, None)
        } else if len <= u8::MAX as u64 {
            (24, Some(len.to_be_bytes()[7..8].to_vec()))
        } else if len <= u16::MAX as u64 {
            (25, Some((len as u16).to_be_bytes().to_vec()))
        } else if len <= u32::MAX as u64 {
            (26, Some((len as u32).to_be_bytes().to_vec()))
        } else {
            (27, Some(len.to_be_bytes().to_vec()))
        };
        self.push_u8((major << 5) | info)?;
        if let Some(bytes) = extra {
            self.push(&bytes)?;
        }
        Ok(())
    }

    fn push_u8(&mut self, value: u8) -> Result<(), CborError> {
        self.push(&[value])
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), CborError> {
        if self.buffer.len().saturating_add(bytes.len()) > self.max_len {
            return Err(CborError::TooLarge);
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }
}

fn log_watch_throttle(audit: &mut dyn AuditSink, delay_ms: u64) {
    let mut line = HeaplessString::<128>::new();
    let _ = write!(
        line,
        "observe ingest.watch throttled delay_ms={} min_interval_ms={}",
        delay_ms,
        OBSERVE_WATCH_MIN_INTERVAL_MS
    );
    audit.info(line.as_str());
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

fn validate_bind_path(target: &str) -> Result<(), NineDoorBridgeError> {
    validate_action_target(target)
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
    let segments = split_path_segments(path);
    let sharding = generated::sharding_config();
    if sharding.enabled {
        if let ["shard", label, "worker", worker_id, leaf] = segments.as_slice() {
            if *leaf != WORKER_TELEMETRY_FILE {
                return None;
            }
            if !shard_label_known(label) {
                return None;
            }
            let expected = worker_shard_label(worker_id, sharding);
            if expected != *label {
                return None;
            }
            return Some(worker_id);
        }
        if legacy_worker_alias_enabled(sharding) {
            if let ["worker", worker_id, leaf] = segments.as_slice() {
                if *leaf == WORKER_TELEMETRY_FILE {
                    return Some(worker_id);
                }
            }
        }
        return None;
    }
    if let ["worker", worker_id, leaf] = segments.as_slice() {
        if *leaf == WORKER_TELEMETRY_FILE {
            return Some(worker_id);
        }
    }
    None
}

fn parse_cas_path(path: &str) -> Result<Option<CasPath>, NineDoorBridgeError> {
    let segments = split_path_segments(path);
    match segments.as_slice() {
        ["updates"] => return Ok(Some(CasPath::UpdatesRoot)),
        ["models"] => return Ok(Some(CasPath::ModelsRoot)),
        _ => {}
    }
    match segments.as_slice() {
        ["updates", epoch] => {
            validate_epoch(epoch)?;
            return Ok(Some(CasPath::UpdateEpoch {
                epoch: (*epoch).to_owned(),
            }));
        }
        ["updates", epoch, "manifest.cbor"] => {
            validate_epoch(epoch)?;
            return Ok(Some(CasPath::UpdateManifest {
                epoch: (*epoch).to_owned(),
            }));
        }
        ["updates", epoch, "status"] => {
            validate_epoch(epoch)?;
            return Ok(Some(CasPath::UpdateStatus {
                epoch: (*epoch).to_owned(),
                cbor: false,
            }));
        }
        ["updates", epoch, "status.cbor"] => {
            validate_epoch(epoch)?;
            return Ok(Some(CasPath::UpdateStatus {
                epoch: (*epoch).to_owned(),
                cbor: true,
            }));
        }
        ["updates", epoch, "chunks"] => {
            validate_epoch(epoch)?;
            return Ok(Some(CasPath::UpdateChunks {
                epoch: (*epoch).to_owned(),
            }));
        }
        ["updates", epoch, "chunks", digest] => {
            validate_epoch(epoch)?;
            return Ok(Some(CasPath::UpdateChunk {
                epoch: (*epoch).to_owned(),
                digest: parse_sha256(digest)?,
            }));
        }
        _ => {}
    }
    match segments.as_slice() {
        ["models", digest] => {
            return Ok(Some(CasPath::ModelRoot {
                digest: parse_sha256(digest)?,
            }));
        }
        ["models", digest, "weights"] => {
            return Ok(Some(CasPath::ModelFile {
                digest: parse_sha256(digest)?,
                kind: ModelFileKind::Weights,
            }));
        }
        ["models", digest, "schema"] => {
            return Ok(Some(CasPath::ModelFile {
                digest: parse_sha256(digest)?,
                kind: ModelFileKind::Schema,
            }));
        }
        ["models", digest, "signature"] => {
            return Ok(Some(CasPath::ModelFile {
                digest: parse_sha256(digest)?,
                kind: ModelFileKind::Signature,
            }));
        }
        _ => {}
    }
    Ok(None)
}

fn validate_epoch(epoch: &str) -> Result<(), NineDoorBridgeError> {
    let trimmed = epoch.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_EPOCH_LEN {
        return Err(NineDoorBridgeError::InvalidPath);
    }
    if !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(NineDoorBridgeError::InvalidPath);
    }
    Ok(())
}

fn parse_sha256(hex_str: &str) -> Result<[u8; 32], NineDoorBridgeError> {
    if hex_str.len() != 64 || !hex_str.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(NineDoorBridgeError::InvalidPath);
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(hex_str.as_bytes(), &mut out)
        .map_err(|_| NineDoorBridgeError::InvalidPath)?;
    Ok(out)
}

fn decode_cas_payload(data: &[u8]) -> Result<Vec<u8>, NineDoorBridgeError> {
    let trimmed = trim_payload(data);
    if let Some(encoded) = trimmed.strip_prefix(b"b64:") {
        return BASE64_STANDARD
            .decode(encoded)
            .map_err(|_| NineDoorBridgeError::InvalidPayload);
    }
    Ok(trimmed.to_vec())
}

fn trim_payload(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    if end > 0 && data[end - 1] == b'\n' {
        end -= 1;
        if end > 0 && data[end - 1] == b'\r' {
            end -= 1;
        }
    }
    &data[..end]
}

fn append_sidecar_bounded(
    buffer: &mut Vec<u8>,
    data: &[u8],
    max_bytes: usize,
) -> Result<u32, NineDoorBridgeError> {
    if buffer.len().saturating_add(data.len()) > max_bytes {
        return Err(NineDoorBridgeError::BufferFull);
    }
    buffer.extend_from_slice(data);
    Ok(data.len() as u32)
}

fn push_bounded_line(out: &mut String, line: &str, max_bytes: usize) -> bool {
    if out.len().saturating_add(line.len()) > max_bytes {
        return false;
    }
    out.push_str(line);
    true
}

fn render_spool_status(spool: &OfflineSpool, max_bytes: usize) -> Vec<u8> {
    let config = spool.config();
    let entries: Vec<SpoolFrame> = spool.snapshot();
    let mut out = String::new();
    let summary = format!(
        "entries={} bytes={} max_entries={} max_bytes={}\n",
        entries.len(),
        spool.buffered_bytes(),
        config.max_entries,
        config.max_bytes
    );
    let _ = push_bounded_line(&mut out, &summary, max_bytes);
    for frame in entries {
        let payload = String::from_utf8_lossy(&frame.payload);
        let line = format!(
            "seq={} bytes={} payload={}\n",
            frame.seq,
            frame.payload.len(),
            payload
        );
        if !push_bounded_line(&mut out, &line, max_bytes) {
            break;
        }
    }
    out.into_bytes()
}

fn render_tamper_log(entries: Vec<TamperEntry>, max_bytes: usize) -> Vec<u8> {
    let mut out = String::new();
    for entry in entries {
        let reason = match entry.reason {
            TamperReason::PayloadOversize => "payload-oversize",
            TamperReason::DutyCycleExceeded => "duty-cycle",
        };
        let line = format!(
            "tamper ts_ms={} reason={} bytes={}\n",
            entry.timestamp_ms, reason, entry.payload_bytes
        );
        if !push_bounded_line(&mut out, &line, max_bytes) {
            break;
        }
    }
    out.into_bytes()
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

fn ensure_line_terminated(data: &[u8]) -> Vec<u8> {
    if data.ends_with(b"\n") {
        return data.to_vec();
    }
    let mut out = data.to_vec();
    out.push(b'\n');
    out
}

fn validate_json_lines(payload: &str) -> Result<(), NineDoorBridgeError> {
    for line in payload.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        validate_json_envelope(trimmed)?;
    }
    Ok(())
}

fn parse_replay_command(payload: &str) -> Result<ReplayCommand, NineDoorBridgeError> {
    validate_json_envelope(payload)?;
    let from = parse_json_u64_field(payload, "from").ok_or(NineDoorBridgeError::InvalidPayload)?;
    Ok(ReplayCommand { from })
}

fn parse_json_u64_field(input: &str, key: &str) -> Option<u64> {
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
        let mut end = 0usize;
        for ch in rest.chars() {
            if !ch.is_ascii_digit() {
                break;
            }
            end = end.saturating_add(ch.len_utf8());
        }
        if end == 0 {
            return None;
        }
        return rest[..end].parse().ok();
    }
    None
}

fn escape_json_string(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn error_code_for_audit(error: &NineDoorBridgeError) -> ErrorCode {
    match error {
        NineDoorBridgeError::Permission => ErrorCode::Permission,
        NineDoorBridgeError::InvalidPath => ErrorCode::NotFound,
        NineDoorBridgeError::BufferFull => ErrorCode::TooBig,
        NineDoorBridgeError::InvalidPayload
        | NineDoorBridgeError::Unsupported(_)
        | NineDoorBridgeError::AttachTimeout => ErrorCode::Invalid,
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
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
    if payload.contains("\"bind\"") {
        let from =
            parse_json_string_field(payload, "from").ok_or(NineDoorBridgeError::InvalidPayload)?;
        let to =
            parse_json_string_field(payload, "to").ok_or(NineDoorBridgeError::InvalidPayload)?;
        return Ok(QueenCtlCommand::Bind { from, to });
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

fn log_audit_wrap(label: &str, dropped_bytes: u64, new_base: u64) {
    let mut line = HeaplessString::<TELEMETRY_AUDIT_LINE>::new();
    let _ = write!(
        line,
        "audit {} truncation dropped_bytes={} new_base={}",
        label, dropped_bytes, new_base
    );
    log_buffer::append_log_line(line.as_str());
    log_buffer::append_user_line(line.as_str());
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
