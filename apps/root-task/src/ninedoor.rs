// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Minimal in-kernel NineDoor bridge for console-driven control and log access.
// Author: Lukas Bower

#![cfg(feature = "kernel")]
#![allow(dead_code)]

extern crate alloc;

#[cfg(not(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)))]
use crate::affinity;
use crate::authority::{AuthorityError, AuthorityOp, AuthorityQueue};
use crate::bootstrap::{boot_tracer, log as boot_log, BootPhase};
use crate::critical_tcb::FaultClass;
use crate::event::AuditSink;
use crate::generated;
use crate::lifecycle;
use crate::log_buffer;
#[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
use crate::ninedoor_service::NamespaceTransportFailureStage;
use crate::ninedoor_service::{NamespaceServiceBoundary, NamespaceTransportFailureEvidence};
use crate::observe::IngestSnapshot;
use crate::serial::DEFAULT_LINE_CAPACITY;
#[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
use crate::{
    hal::{
        ninedoor_service::NineDoorServiceRuntime,
        worker_task::{
            enqueue_target_worker_kill, enqueue_target_worker_operation,
            enqueue_target_worker_spawn, target_worker_namespace_snapshot_by_public_id,
            target_worker_namespace_snapshot_for_identity, target_worker_namespace_snapshots,
            TargetWorkerNamespaceSnapshot,
        },
        HalError, KernelHal,
    },
    ninedoor_service::NineDoorContainmentTurn,
    worker_supervisor::{flat_slot_index, MAX_EXECUTABLE_WORKER_SLOTS},
};
use alloc::{
    borrow::ToOwned,
    collections::{BTreeMap, VecDeque},
    format,
    string::{String, ToString},
    vec::Vec,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use cohesix_cas::{CasManifest, CasManifestError, CAS_MANIFEST_MAX_CHUNKS, CAS_MANIFEST_SCHEMA};
use cohesix_ticket::TicketToken;
use core::fmt::{self, Write};
use core::str;
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
use core::sync::atomic::{AtomicBool, Ordering};
use ed25519_dalek::{Signature, VerifyingKey};
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use secure9p_codec::ErrorCode;
#[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
use secure9p_transport::TransportState;
use secure9p_transport::{NamespaceOpcode, TransportError};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use sidecar_bus::{LinkState, OfflineSpool, SpoolConfig, SpoolError, SpoolFrame};
use signature::Verifier;
use worker_task_abi::{
    Digest32, ReceiptDigests, WorkerAction, WorkerControlRecord, WorkerIdentity, WorkerOutcome,
    WorkerRole,
};

#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
static NINEDOOR_QEMU_EVIDENCE_LOCAL_REVOKE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Stable QEMU observation point after one donated NineDoor Call has returned.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
pub extern "C" fn cohesix_ninedoor_qemu_evidence_post_prepare() {
    core::hint::black_box(cohesix_ninedoor_qemu_evidence_request_local_revoke as extern "C" fn());
}

/// Request one root-local transport revoke from the ordinary preparation path.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
pub extern "C" fn cohesix_ninedoor_qemu_evidence_request_local_revoke() {
    NINEDOOR_QEMU_EVIDENCE_LOCAL_REVOKE_REQUESTED.store(true, Ordering::Release);
    core::hint::black_box(());
}

#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
fn take_ninedoor_qemu_evidence_local_revoke_request() -> bool {
    NINEDOOR_QEMU_EVIDENCE_LOCAL_REVOKE_REQUESTED.swap(false, Ordering::AcqRel)
}

const LOG_PATH: &str = "/log/queen.log";
const QUEEN_CTL_PATH: &str = "/queen/ctl";
const QUEEN_SCHEDULE_ROOT_PATH: &str = "/queen/schedule";
const QUEEN_SCHEDULE_CTL_PATH: &str = "/queen/schedule/ctl";
const QUEEN_LEASE_ROOT_PATH: &str = "/queen/lease";
const QUEEN_LEASE_CTL_PATH: &str = "/queen/lease/ctl";
const QUEEN_EXPORT_ROOT_PATH: &str = "/queen/export";
const QUEEN_EXPORT_CTL_PATH: &str = "/queen/export/ctl";
const BUS_ROOT_PATH: &str = "/bus";
const PROC_BOOT_PATH: &str = "/proc/boot";
const PROC_TESTS_PATH: &str = "/proc/tests";
const PROC_TESTS_QUICK_PATH: &str = "/proc/tests/selftest_quick.coh";
const PROC_TESTS_FULL_PATH: &str = "/proc/tests/selftest_full.coh";
const PROC_TESTS_NEGATIVE_PATH: &str = "/proc/tests/selftest_negative.coh";
const PROC_TESTS_SMP_PATH: &str = "/proc/tests/selftest_smp.coh";
const PROC_INGEST_ROOT_PATH: &str = "/proc/ingest";
const PROC_INGEST_P50_PATH: &str = "/proc/ingest/p50_ms";
const PROC_INGEST_P95_PATH: &str = "/proc/ingest/p95_ms";
const PROC_INGEST_BACKPRESSURE_PATH: &str = "/proc/ingest/backpressure";
const PROC_INGEST_DROPPED_PATH: &str = "/proc/ingest/dropped";
const PROC_INGEST_QUEUED_PATH: &str = "/proc/ingest/queued";
const PROC_INGEST_WATCH_PATH: &str = "/proc/ingest/watch";
const PROC_9P_ROOT_PATH: &str = "/proc/9p";
const PROC_9P_SESSION_ROOT_PATH: &str = "/proc/9p/session";
const PROC_9P_SESSION_ACTIVE_PATH: &str = "/proc/9p/session/active";
const PROC_LIFECYCLE_ROOT_PATH: &str = "/proc/lifecycle";
const PROC_LIFECYCLE_STATE_PATH: &str = "/proc/lifecycle/state";
const PROC_LIFECYCLE_REASON_PATH: &str = "/proc/lifecycle/reason";
const PROC_LIFECYCLE_SINCE_PATH: &str = "/proc/lifecycle/since";
const PROC_ROOT_ROOT_PATH: &str = "/proc/root";
const PROC_ROOT_REACHABLE_PATH: &str = "/proc/root/reachable";
const PROC_ROOT_LAST_SEEN_PATH: &str = "/proc/root/last_seen_ms";
const PROC_ROOT_CUT_REASON_PATH: &str = "/proc/root/cut_reason";
const PROC_PRESSURE_ROOT_PATH: &str = "/proc/pressure";
const PROC_PRESSURE_BUSY_PATH: &str = "/proc/pressure/busy";
const PROC_PRESSURE_QUOTA_PATH: &str = "/proc/pressure/quota";
const PROC_PRESSURE_CUT_PATH: &str = "/proc/pressure/cut";
const PROC_PRESSURE_POLICY_PATH: &str = "/proc/pressure/policy";
const PROC_SCHEDULE_ROOT_PATH: &str = "/proc/schedule";
const PROC_SCHEDULE_SUMMARY_PATH: &str = "/proc/schedule/summary";
const PROC_SCHEDULE_QUEUE_PATH: &str = "/proc/schedule/queue";
#[cfg(feature = "release-qemu")]
const PROC_SCHEDULE_QEMU_FLIGHT_PATH: &str = "/proc/schedule/qemu-flight";
const PROC_LEASE_ROOT_PATH: &str = "/proc/lease";
const PROC_LEASE_SUMMARY_PATH: &str = "/proc/lease/summary";
const PROC_LEASE_ACTIVE_PATH: &str = "/proc/lease/active";
const PROC_LEASE_BY_ID_PATH: &str = "/proc/lease/by-id";
const PROC_LEASE_BY_ID_PREFIX: &str = "/proc/lease/by-id/";
const PROC_LEASE_PREEMPTIONS_PATH: &str = "/proc/lease/preemptions";
const BOOT_HEADER: &str = "Cohesix boot: root-task online";
const MAX_STREAM_LINES: usize = log_buffer::LOG_SNAPSHOT_LINES;
const MAX_WORKERS: usize = 1500;
const MAX_BINDS: usize = 8;
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
const QUEEN_LIFECYCLE_ROOT_PATH: &str = "/queen/lifecycle";
const QUEEN_LIFECYCLE_CTL_PATH: &str = "/queen/lifecycle/ctl";
const MAX_POLICY_PATH_COMPONENTS: usize = 8;
const MAX_ACTION_ID_LEN: usize = 64;
const MAX_SCHEDULE_ID_LEN: usize = 64;
const MAX_SCHEDULE_ROLE_LEN: usize = 16;
const MAX_LEASE_ID_LEN: usize = 32;
const MAX_LEASE_SUBJECT_LEN: usize = 32;
const MAX_LEASE_RESOURCE_LEN: usize = 48;
const MAX_LEASE_REASON_LEN: usize = 24;
const LEASE_REQUEST_TAG_BYTES: usize = 16;
const MAX_POLICY_REV_ID_LEN: usize = 64;
const MAX_EXPORT_ID_LEN: usize = 64;
const HOST_TICKET_ID_MAX_BYTES: usize = 128;
const HOST_TICKET_WORKER_ID_MAX_BYTES: usize = 32;
const HOST_TICKET_REASON_MAX_BYTES: usize = 128;
const HOST_TICKET_V2_REQUEST_SCHEMA: &str = "host-ticket/v2";
const HOST_TICKET_V2_RESULT_SCHEMA: &str = "host-ticket-result/v2";
const HOST_TICKET_CURRENT_SCHEMA: &str = "host-ticket-current/v1";
const HOST_TICKET_CURRENT_PREFIX: &str = "/host/tickets/current/";
const HOST_TICKET_MAX_ADMISSIONS: usize = 256;
const HOST_TICKET_LOG_MAX_BYTES: usize = MAX_STREAM_LINES * DEFAULT_LINE_CAPACITY;
const CAT_CHUNK_PREFIX: &str = "C1:";
const CAT_CHUNK_TEXT_BYTES: usize = 176;
const GPU_LEASE_SCHEMA: &str = "gpu-lease/v1";
const GPU_LEASE_ACTIVE_STATE: &str = "ACTIVE";
const LEASE_STATE_ACTIVE: &str = "ACTIVE";
const GPU_CTL_MAX_BYTES: u32 = 1024;
const GPU_LEASE_MAX_BYTES: u32 = 1024;
const GPU_STATUS_MAX_BYTES: u32 = UI_MAX_STREAM_BYTES as u32;
const GPU_BRIDGE_CTL_MAX_BYTES: u32 = 128 * 1024;
const GPU_BRIDGE_STATUS_MAX_BYTES: usize = 512;
const GPU_BRIDGE_MAX_BYTES: usize = 128 * 1024;
const GPU_BRIDGE_WIRE_SCHEMA: &str = "gpu-bridge-snapshot/v2";
const GPU_BRIDGE_MAX_TTL_MS: u64 = 60_000;
const GPU_BRIDGE_EMPTY_VALUE: &str = "-";
const GPU_MODELS_ACTIVE_MAX_BYTES: u32 = 4096;
const GPU_MODEL_MANIFEST_MAX_BYTES: usize = 8 * 1024;
const GPU_MODEL_ID_MAX_BYTES: usize = 128;
const GPU_TELEMETRY_SCHEMA_MAX_BYTES: usize = 4096;
const QEMU_LORA_EXPORT_JOB_ID: &str = "qemu-evidence-job";
const QEMU_LORA_EXPORT_TELEMETRY: &[u8] = b"aa";
const QEMU_LORA_EXPORT_BASE_MODEL: &[u8] = b"fixture-base-model";
const QEMU_LORA_EXPORT_POLICY: &[u8] =
    b"source = \"fixture\"\nprofile = \"qemu\"\nproduction = false\n";
const OBSERVE_P50_BYTES: usize = generated::OBSERVABILITY_CONFIG.proc_ingest.p50_ms_bytes as usize;
const OBSERVE_P95_BYTES: usize = generated::OBSERVABILITY_CONFIG.proc_ingest.p95_ms_bytes as usize;
const OBSERVE_BACKPRESSURE_BYTES: usize = generated::OBSERVABILITY_CONFIG
    .proc_ingest
    .backpressure_bytes as usize;
const OBSERVE_DROPPED_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.dropped_bytes as usize;
const OBSERVE_QUEUED_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.queued_bytes as usize;
const OBSERVE_WATCH_MAX_ENTRIES: usize = generated::OBSERVABILITY_CONFIG
    .proc_ingest
    .watch_max_entries as usize;
const OBSERVE_WATCH_LINE_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.watch_line_bytes as usize;
const OBSERVE_WATCH_MIN_INTERVAL_MS: u64 = generated::OBSERVABILITY_CONFIG
    .proc_ingest
    .watch_min_interval_ms as u64;
const OBSERVE_ROOT_REACHABLE_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_root.reachable_bytes as usize;
const OBSERVE_ROOT_LAST_SEEN_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_root.last_seen_ms_bytes as usize;
const OBSERVE_ROOT_CUT_REASON_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_root.cut_reason_bytes as usize;
const OBSERVE_PRESSURE_BUSY_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_pressure.busy_bytes as usize;
const OBSERVE_PRESSURE_QUOTA_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_pressure.quota_bytes as usize;
const OBSERVE_PRESSURE_CUT_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_pressure.cut_bytes as usize;
const OBSERVE_PRESSURE_POLICY_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_pressure.policy_bytes as usize;
const OBSERVE_SCHEDULE_SUMMARY_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_schedule.summary_bytes as usize;
const OBSERVE_SCHEDULE_QUEUE_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_schedule.queue_bytes as usize;
const OBSERVE_LEASE_SUMMARY_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_lease.summary_bytes as usize;
const OBSERVE_LEASE_ACTIVE_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_lease.active_bytes as usize;
const OBSERVE_LEASE_PREEMPTIONS_BYTES: usize =
    generated::OBSERVABILITY_CONFIG.proc_lease.preemptions_bytes as usize;
const SIDECAR_LOG_MAX_BYTES: usize = generated::SECURE9P_LIMITS.msize as usize;
const TELEMETRY_INGEST_RECORD_MAX_BYTES: usize = 4096;
const TELEMETRY_INGEST_INITIAL_RECORD_SLOTS: usize = 4;
const TELEMETRY_REFERENCE_CHUNK_SCHEMA: &str = "coh-ref-c/v1";
const TELEMETRY_REFERENCE_DIGEST_MAX_BYTES: usize = 64;
const AUTHORITY_QUEUE_MAX: usize = 16;

/// One compact NineDoor containment record retained across Recovery turns.
///
/// Recovery only copies these scalar fields. Formatting and serial admission
/// are deferred to an ordinary retained-output turn after containment yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NineDoorContainmentDiagnostic {
    Fault {
        expected_generation: u64,
        observed_generation: u32,
        fault_class: FaultClass,
        sequence: u64,
    },
    InvalidMailbox {
        generation: u64,
    },
    TransportRevoked {
        generation: u64,
        evidence: NamespaceTransportFailureEvidence,
    },
    ContainmentFailed {
        generation: u64,
    },
    IncompleteProof {
        generation: u64,
    },
    Teardown {
        generation: u64,
    },
}

impl NineDoorContainmentDiagnostic {
    /// Render the retained record only after ordinary Operator output admits it.
    pub(crate) fn render(self) -> Result<HeaplessString<DEFAULT_LINE_CAPACITY>, fmt::Error> {
        let mut line = HeaplessString::new();
        match self {
            Self::Fault {
                expected_generation,
                observed_generation,
                fault_class,
                sequence,
            } if expected_generation == u64::from(observed_generation) => write!(
                line,
                "[ninedoor-service] generation={expected_generation} terminal-fault class={fault_class:?} sequence={sequence}"
            )?,
            Self::Fault {
                expected_generation,
                observed_generation,
                ..
            } => write!(
                line,
                "[ninedoor-service] fault generation mismatch expected={expected_generation} observed={observed_generation}"
            )?,
            Self::InvalidMailbox { generation } => write!(
                line,
                "[ninedoor-service] generation={generation} invalid fault mailbox action=contain"
            )?,
            Self::TransportRevoked {
                generation,
                evidence,
            } => write!(
                line,
                "[ninedoor-service] generation={generation} terminal-revoke state=local reason={:?} stage={:?} expected_sequence={} observed_sequence={}",
                evidence.error,
                evidence.stage,
                evidence.expected_sequence,
                evidence.observed_sequence,
            )?,
            Self::ContainmentFailed { generation } => write!(
                line,
                "[ninedoor-service] terminal containment failed generation={generation} action=quarantine-no-replacement"
            )?,
            Self::IncompleteProof { generation } => write!(
                line,
                "[ninedoor-service] terminal containment proof incomplete generation={generation} action=quarantine-no-replacement"
            )?,
            Self::Teardown { generation } => write!(
                line,
                "NINEDOOR_SERVICE_TEARDOWN generation={generation} tcb_suspended=yes mappings_scrubbed=yes recovery_reply_revoked=yes capabilities_revoked=yes generation_fenced=yes state=terminal"
            )?,
        }
        Ok(line)
    }
}

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
const SELFTEST_SMP_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_smp.coh"
));

/// Root-owned NineDoor policy and mutation bridge behind the typed parser boundary.
#[derive(Debug)]
pub struct NineDoorBridge {
    namespace_service: NamespaceServiceBoundary,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_service: Option<NineDoorServiceRuntime>,
    pending_containment_fault_diagnostic: Option<NineDoorContainmentDiagnostic>,
    pending_containment_failure_diagnostic: Option<NineDoorContainmentDiagnostic>,
    pending_containment_teardown_diagnostic: Option<NineDoorContainmentDiagnostic>,
    attached: bool,
    session_role: Option<SessionRoleLabel>,
    session_ticket: Option<String>,
    session_scope: Option<String>,
    retired_session_ticket: Option<String>,
    retired_session_scope: Option<String>,
    ui: generated::UiProviderConfig,
    telemetry: generated::TelemetryConfig,
    telemetry_ingest: TelemetryIngestState,
    workers: Vec<WorkerTelemetry>,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_worker_indexes: [Option<usize>; MAX_EXECUTABLE_WORKER_SLOTS],
    binds: HeaplessVec<BindEntry, MAX_BINDS>,
    retired_session_binds: HeaplessVec<BindEntry, MAX_BINDS>,
    authority: AuthorityQueue,
    host: HostState,
    gpu: GpuState,
    sidecars: SidecarState,
    policy: PolicyState,
    schedule: ScheduleState,
    lease: LeaseState,
    export: ExportState,
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
    /// Authority queue is saturated.
    Busy,
}

/// Outcome for a successful append-style `ECHO` operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EchoOutcome {
    /// The payload was appended without additional caller-facing metadata.
    Appended,
    /// A telemetry-ingest control write created a new segment.
    TelemetrySegmentCreated { seg_id: String },
}

impl EchoOutcome {
    /// Return the created telemetry segment id when this append allocated one.
    pub fn telemetry_segment_id(&self) -> Option<&str> {
        match self {
            Self::Appended => None,
            Self::TelemetrySegmentCreated { seg_id } => Some(seg_id.as_str()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct TelemetryTail {
    pub(crate) lines: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    pub(crate) start_offset: u64,
    pub(crate) consumed_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TelemetryTailMeta {
    pub(crate) start_offset: u64,
    pub(crate) consumed_bytes: usize,
}

#[derive(Debug, Clone)]
struct TelemetryIngestSegment {
    id: String,
    bytes: usize,
    data: Vec<u8>,
    mode: TelemetryIngestSegmentMode,
    reference_entries: usize,
    reference_manifest_bytes: usize,
    reference_total_bytes: u64,
    reference_next_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelemetryIngestSegmentMode {
    Unknown,
    Plain,
    ReferenceManifest,
}

#[derive(Debug)]
struct TelemetryIngestDevice {
    next_id: u64,
    total_bytes: usize,
    latest: Option<String>,
    segments: VecDeque<TelemetryIngestSegment>,
}

impl TelemetryIngestDevice {
    fn new() -> Self {
        Self {
            next_id: 1,
            total_bytes: 0,
            latest: None,
            segments: VecDeque::new(),
        }
    }

    fn allocate_id(&mut self) -> String {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        format!("seg-{:06}", id)
    }
}

#[derive(Debug)]
struct TelemetryIngestState {
    config: generated::TelemetryIngestConfig,
    devices: BTreeMap<String, TelemetryIngestDevice>,
}

impl TelemetryIngestState {
    fn new() -> Self {
        Self {
            config: generated::telemetry_ingest_config(),
            devices: BTreeMap::new(),
        }
    }

    fn enabled(&self) -> bool {
        self.config.max_segments_per_device > 0
            && self.config.max_bytes_per_segment > 0
            && self.config.max_total_bytes_per_device > 0
    }

    fn ensure_device_mut(&mut self, device_id: &str) -> &mut TelemetryIngestDevice {
        self.devices
            .entry(device_id.to_owned())
            .or_insert_with(TelemetryIngestDevice::new)
    }

    fn device(&self, device_id: &str) -> Option<&TelemetryIngestDevice> {
        self.devices.get(device_id)
    }

    fn initial_segment_capacity(&self) -> usize {
        let max_segment_bytes = self.config.max_bytes_per_segment as usize;
        max_segment_bytes.min(
            TELEMETRY_INGEST_RECORD_MAX_BYTES.saturating_mul(TELEMETRY_INGEST_INITIAL_RECORD_SLOTS),
        )
    }

    fn create_segment(&mut self, device_id: &str) -> Result<String, NineDoorBridgeError> {
        if !self.enabled() {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let max_segments = self.config.max_segments_per_device as usize;
        let eviction_policy = self.config.eviction_policy;
        let initial_capacity = self.initial_segment_capacity();
        if max_segments == 0 {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let device = self.ensure_device_mut(device_id);
        if device.segments.len().saturating_add(1) > max_segments {
            match eviction_policy {
                generated::TelemetryIngestEvictionPolicy::Refuse => {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                generated::TelemetryIngestEvictionPolicy::EvictOldest => {
                    while device.segments.len().saturating_add(1) > max_segments {
                        if let Some(segment) = device.segments.pop_front() {
                            device.total_bytes = device.total_bytes.saturating_sub(segment.bytes);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        let seg_id = device.allocate_id();
        device.latest = Some(seg_id.clone());
        device.segments.push_back(TelemetryIngestSegment {
            id: seg_id.clone(),
            bytes: 0,
            data: Vec::with_capacity(initial_capacity),
            mode: TelemetryIngestSegmentMode::Unknown,
            reference_entries: 0,
            reference_manifest_bytes: 0,
            reference_total_bytes: 0,
            reference_next_seq: 1,
        });
        Ok(seg_id)
    }

    fn append_record(
        &mut self,
        device_id: &str,
        seg_id: &str,
        payload: &str,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.enabled() {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let payload_bytes = payload.as_bytes();
        let needs_newline = !payload_bytes.ends_with(b"\n");
        let record_len = payload_bytes
            .len()
            .saturating_add(if needs_newline { 1 } else { 0 });
        if record_len > TELEMETRY_INGEST_RECORD_MAX_BYTES {
            return Err(NineDoorBridgeError::BufferFull);
        }
        let max_segment_bytes = self.config.max_bytes_per_segment as usize;
        let max_total_bytes = self.config.max_total_bytes_per_device as usize;
        let max_reference_entries = self.config.max_reference_entries_per_segment as usize;
        let max_reference_manifest_bytes =
            self.config.max_reference_manifest_bytes_per_segment as usize;
        let max_reference_bytes = self.config.max_reference_bytes_per_segment;
        let eviction_policy = self.config.eviction_policy;
        let reference_chunk = parse_telemetry_reference_chunk(payload)?;
        let device = self
            .devices
            .get_mut(device_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let mut segment_index = device
            .segments
            .iter()
            .position(|segment| segment.id.as_str() == seg_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let segment_bytes = device
            .segments
            .get(segment_index)
            .map(|segment| segment.bytes)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        if segment_bytes.saturating_add(record_len) > max_segment_bytes {
            return Err(NineDoorBridgeError::BufferFull);
        }
        let total_after = device.total_bytes.saturating_add(record_len);
        if total_after > max_total_bytes {
            match eviction_policy {
                generated::TelemetryIngestEvictionPolicy::Refuse => {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                generated::TelemetryIngestEvictionPolicy::EvictOldest => {
                    let needed = total_after.saturating_sub(max_total_bytes);
                    let mut freed = 0usize;
                    let mut scan = 0usize;
                    while freed < needed && scan < device.segments.len() {
                        if device.segments.get(scan).map(|seg| seg.id.as_str()) == Some(seg_id) {
                            scan = scan.saturating_add(1);
                            continue;
                        }
                        if let Some(segment) = device.segments.remove(scan) {
                            if scan < segment_index {
                                segment_index = segment_index.saturating_sub(1);
                            }
                            if device.latest.as_deref() == Some(segment.id.as_str()) {
                                device.latest = device.segments.back().map(|seg| seg.id.clone());
                            }
                            device.total_bytes = device.total_bytes.saturating_sub(segment.bytes);
                            freed = freed.saturating_add(segment.bytes);
                            continue;
                        }
                        break;
                    }
                    if freed < needed {
                        return Err(NineDoorBridgeError::BufferFull);
                    }
                }
            }
        }
        let segment = device
            .segments
            .get_mut(segment_index)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        if segment.id.as_str() != seg_id {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        match reference_chunk {
            Some(reference) => {
                if matches!(segment.mode, TelemetryIngestSegmentMode::Plain) {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if reference.seq != segment.reference_next_seq {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if reference.offset != segment.reference_total_bytes {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if segment.reference_entries.saturating_add(1) > max_reference_entries {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                if segment.reference_manifest_bytes.saturating_add(record_len)
                    > max_reference_manifest_bytes
                {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                let referenced_total = segment
                    .reference_total_bytes
                    .saturating_add(reference.chunk_bytes);
                if referenced_total > max_reference_bytes {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                segment.mode = TelemetryIngestSegmentMode::ReferenceManifest;
                segment.reference_entries = segment.reference_entries.saturating_add(1);
                segment.reference_manifest_bytes =
                    segment.reference_manifest_bytes.saturating_add(record_len);
                segment.reference_total_bytes = referenced_total;
                segment.reference_next_seq = segment.reference_next_seq.saturating_add(1);
            }
            None => {
                if matches!(segment.mode, TelemetryIngestSegmentMode::ReferenceManifest) {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if matches!(segment.mode, TelemetryIngestSegmentMode::Unknown) {
                    segment.mode = TelemetryIngestSegmentMode::Plain;
                }
            }
        }
        segment.data.extend_from_slice(payload_bytes);
        if needs_newline {
            segment.data.push(b'\n');
        }
        segment.bytes = segment.bytes.saturating_add(record_len);
        device.total_bytes = device.total_bytes.saturating_add(record_len);
        Ok(())
    }
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
            Self::Busy => write!(f, "busy"),
        }
    }
}

fn namespace_transport_error(error: TransportError) -> NineDoorBridgeError {
    match error {
        TransportError::InvalidPath => NineDoorBridgeError::InvalidPath,
        TransportError::InvalidPayload
        | TransportError::InvalidOperation
        | TransportError::InvalidAbi
        | TransportError::InvalidLimits
        | TransportError::InvalidFrameLength
        | TransportError::FrameTooLarge
        | TransportError::PartialFrame
        | TransportError::BufferTooSmall => NineDoorBridgeError::InvalidPayload,
        TransportError::QueueFull
        | TransportError::Closed
        | TransportError::Revoked
        | TransportError::UnknownRequest
        | TransportError::StaleIdentity
        | TransportError::ShortWriteExhausted => NineDoorBridgeError::Busy,
    }
}

#[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
fn worker_target_error(
    error: crate::worker_supervisor::WorkerSupervisorError,
) -> NineDoorBridgeError {
    use crate::worker_supervisor::WorkerSupervisorError;

    match error {
        WorkerSupervisorError::SlotBusy | WorkerSupervisorError::ControlBusy => {
            NineDoorBridgeError::Busy
        }
        WorkerSupervisorError::RoleNotExecutable | WorkerSupervisorError::NotEnabled => {
            NineDoorBridgeError::Permission
        }
        WorkerSupervisorError::InvalidRecord
        | WorkerSupervisorError::InvalidGeneration
        | WorkerSupervisorError::InvalidState
        | WorkerSupervisorError::InvalidImage => NineDoorBridgeError::InvalidPayload,
        WorkerSupervisorError::Backend | WorkerSupervisorError::ContainmentIncomplete => {
            NineDoorBridgeError::Busy
        }
        WorkerSupervisorError::NoControlPending => NineDoorBridgeError::InvalidPayload,
    }
}

impl NineDoorBridge {
    /// Construct a bridge with the host compatibility boundary. A selected
    /// target remains fail-closed until its supervisor uses
    /// [`Self::with_namespace_service`] with a validated isolated-child
    /// boundary.
    #[must_use]
    pub fn new() -> Self {
        Self::with_namespace_service(NamespaceServiceBoundary::initial())
    }

    /// Construct a bridge around one already admitted namespace-service
    /// generation. Policy and mutation state remain wholly root-owned.
    #[must_use]
    pub fn with_namespace_service(namespace_service: NamespaceServiceBoundary) -> Self {
        #[cfg(feature = "kernel")]
        {
            boot_log::notify_bridge_created();
        }
        let control_plane = generated::control_plane_config();
        let observability = generated::observability_config();
        let host = HostState::new();
        let gpu = GpuState::new(host.enabled && host.has_provider(generated::HostProvider::Nvidia));
        Self {
            namespace_service,
            #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
            target_service: None,
            pending_containment_fault_diagnostic: None,
            pending_containment_failure_diagnostic: None,
            pending_containment_teardown_diagnostic: None,
            attached: false,
            session_role: None,
            session_ticket: None,
            session_scope: None,
            retired_session_ticket: None,
            retired_session_scope: None,
            ui: generated::ui_provider_config(),
            telemetry: generated::telemetry_config(),
            telemetry_ingest: TelemetryIngestState::new(),
            workers: Vec::new(),
            #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
            target_worker_indexes: [None; MAX_EXECUTABLE_WORKER_SLOTS],
            binds: HeaplessVec::new(),
            retired_session_binds: HeaplessVec::new(),
            authority: AuthorityQueue::new(AUTHORITY_QUEUE_MAX),
            host,
            gpu,
            sidecars: SidecarState::new(),
            policy: PolicyState::new(),
            schedule: ScheduleState::new(control_plane.schedule, observability.proc_schedule),
            lease: LeaseState::new(control_plane.lease, observability.proc_lease),
            export: ExportState::new(control_plane.export),
            audit: AuditState::new(generated::audit_config()),
            replay: ReplayState::new(generated::audit_config()),
            observe: ObserveState::new(),
            cas: CasState::new(generated::cas_config()),
        }
    }

    /// Construct the operational target bridge around one already constructed
    /// suspended passive child. Root retains all policy and mutation state.
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    #[must_use]
    pub fn with_target_namespace_service(
        namespace_service: NamespaceServiceBoundary,
        target_service: NineDoorServiceRuntime,
    ) -> Self {
        let mut bridge = Self::with_namespace_service(namespace_service);
        bridge.target_service = Some(target_service);
        bridge
    }

    /// Bootstrap the child to its first receive, prove one parser-only round
    /// trip, then remove the one-shot SC before admitting passive service.
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    pub fn activate_target_service(&mut self) -> Result<(), HalError> {
        let activation_result = self
            .target_service
            .as_mut()
            .ok_or(HalError::Unsupported("ninedoor-target-service-missing"))
            .and_then(NineDoorServiceRuntime::activate);
        if let Err(error) = activation_result {
            self.namespace_service.revoke();
            return Err(error);
        }
        let probe_result = self
            .namespace_service
            .prepare(NamespaceOpcode::Log, "", "")
            .map(|_| ());
        if probe_result.is_err() {
            let close_result = self
                .target_service
                .as_mut()
                .ok_or(HalError::Unsupported("ninedoor-target-service-missing"))
                .and_then(NineDoorServiceRuntime::fail_bootstrap);
            if self.namespace_service.state() != TransportState::Revoked {
                self.namespace_service.revoke();
            }
            return match close_result {
                Ok(()) => Err(HalError::Unsupported("ninedoor-bootstrap-probe")),
                Err(error) => Err(error),
            };
        }
        let finish_result = self
            .target_service
            .as_mut()
            .ok_or(HalError::Unsupported("ninedoor-target-service-missing"))
            .and_then(NineDoorServiceRuntime::finish_bootstrap);
        if let Err(error) = finish_result {
            self.namespace_service.revoke();
            return Err(error);
        }
        Ok(())
    }

    /// Consume one durable service-fault record or a local transport revoke,
    /// then contain the exact old generation without blocking root-control.
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    pub fn contain_target_service_if_faulted(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<NineDoorContainmentTurn, HalError> {
        let Some(runtime) = self.target_service.as_mut() else {
            return Ok(NineDoorContainmentTurn::Idle);
        };
        if !runtime.containment_active() {
            let mut faulted = self.namespace_service.state() == TransportState::Revoked;
            let generation = runtime.generation();
            let mut diagnostic =
                faulted.then_some(NineDoorContainmentDiagnostic::TransportRevoked {
                    generation,
                    evidence: self.namespace_service.revocation_evidence().unwrap_or(
                        NamespaceTransportFailureEvidence {
                            error: TransportError::Revoked,
                            stage: NamespaceTransportFailureStage::ManualRevoke,
                            expected_sequence: 0,
                            observed_sequence: 0,
                        },
                    ),
                });
            match crate::hal::critical_tcb::take_target_service_fault(
                crate::ninedoor_service::SERVICE_TASK_ID,
            ) {
                Ok(Some(record)) => {
                    diagnostic = Some(NineDoorContainmentDiagnostic::Fault {
                        expected_generation: generation,
                        observed_generation: record.identity.supervisor_generation,
                        fault_class: record.fault_class,
                        sequence: record.sequence,
                    });
                    faulted = true;
                }
                Ok(None) => {}
                Err(crate::hal::critical_tcb::CriticalTcbConstructionError::FaultHandoff(
                    crate::critical_tcb::FaultHandoffError::Contended,
                )) => {
                    // Publication is durable. Reserve this Recovery turn and
                    // retry the sole mailbox take without starting teardown.
                    return Ok(NineDoorContainmentTurn::InProgress);
                }
                Err(_) => {
                    diagnostic = Some(NineDoorContainmentDiagnostic::InvalidMailbox { generation });
                    faulted = true;
                }
            }
            if !faulted {
                return Ok(NineDoorContainmentTurn::Idle);
            }
            if self.pending_containment_fault_diagnostic.is_none() {
                self.pending_containment_fault_diagnostic = diagnostic;
            }
            if let Err(error) = runtime.begin_containment(&mut self.namespace_service) {
                if self.pending_containment_failure_diagnostic.is_none() {
                    self.pending_containment_failure_diagnostic =
                        Some(NineDoorContainmentDiagnostic::ContainmentFailed { generation });
                }
                return Err(error);
            }
            // Mailbox consumption and transport/resource fencing own this
            // complete Recovery turn. The persisted cursor selects SuspendTcb
            // only after the sole outer yield replenishes root-control.
            return Ok(NineDoorContainmentTurn::InProgress);
        }

        let generation = runtime.generation();
        let turn = match runtime.contain_one_turn(hal) {
            Ok(turn) => turn,
            Err(error) => {
                if self.pending_containment_failure_diagnostic.is_none() {
                    self.pending_containment_failure_diagnostic =
                        Some(NineDoorContainmentDiagnostic::ContainmentFailed { generation });
                }
                return Err(error);
            }
        };
        match turn {
            NineDoorContainmentTurn::Complete(proof) if proof.complete() => {
                if self.pending_containment_teardown_diagnostic.is_none() {
                    self.pending_containment_teardown_diagnostic =
                        Some(NineDoorContainmentDiagnostic::Teardown { generation });
                }
                self.target_service = None;
            }
            NineDoorContainmentTurn::Complete(_) => {
                if self.pending_containment_failure_diagnostic.is_none() {
                    self.pending_containment_failure_diagnostic =
                        Some(NineDoorContainmentDiagnostic::IncompleteProof { generation });
                }
            }
            NineDoorContainmentTurn::Idle | NineDoorContainmentTurn::InProgress => {}
        }
        Ok(turn)
    }

    /// Side-effect-free local recovery state for retained passive admission.
    ///
    /// Root checks the critical fault mailbox separately. This covers a local
    /// transport revoke and containment already begun on an earlier recovery
    /// turn so neither can be hidden by a retained command's priority.
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    pub(crate) fn target_service_recovery_pending(&self) -> bool {
        self.namespace_service.state() == TransportState::Revoked
            || self
                .target_service
                .as_ref()
                .is_some_and(NineDoorServiceRuntime::containment_active)
            || self.pending_containment_diagnostic().is_some()
    }

    /// Whether one material NineDoor containment unit can advance now.
    /// Retained terminal diagnostics intentionally stay out of this predicate:
    /// they continue to fence passive admission but require an ordinary
    /// bounded output turn to publish and commit them.
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    pub(crate) fn target_service_containment_pending(&self) -> bool {
        self.target_service.as_ref().is_some_and(|runtime| {
            self.namespace_service.state() == TransportState::Revoked
                || runtime.containment_active()
        })
    }

    /// Peek the oldest retained containment record without consuming it.
    #[must_use]
    pub(crate) fn pending_containment_diagnostic(&self) -> Option<NineDoorContainmentDiagnostic> {
        self.pending_containment_fault_diagnostic
            .or(self.pending_containment_failure_diagnostic)
            .or(self.pending_containment_teardown_diagnostic)
    }

    /// Commit one record only after ordinary output admitted its exact copy.
    pub(crate) fn commit_containment_diagnostic(
        &mut self,
        diagnostic: NineDoorContainmentDiagnostic,
    ) {
        if self.pending_containment_fault_diagnostic.is_some() {
            if self.pending_containment_fault_diagnostic == Some(diagnostic) {
                self.pending_containment_fault_diagnostic = None;
            }
            return;
        }
        if self.pending_containment_failure_diagnostic.is_some() {
            if self.pending_containment_failure_diagnostic == Some(diagnostic) {
                self.pending_containment_failure_diagnostic = None;
            }
            return;
        }
        if self.pending_containment_teardown_diagnostic == Some(diagnostic) {
            self.pending_containment_teardown_diagnostic = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn retain_containment_diagnostic_for_test(
        &mut self,
        diagnostic: NineDoorContainmentDiagnostic,
    ) {
        self.pending_containment_fault_diagnostic = Some(diagnostic);
    }

    fn prepare_namespace<'a>(
        &mut self,
        opcode: NamespaceOpcode,
        path: &'a str,
        payload: &'a str,
    ) -> Result<crate::ninedoor_service::PreparedNamespaceView<'a>, NineDoorBridgeError> {
        let prepared = self
            .namespace_service
            .prepare(opcode, path, payload)
            .map_err(namespace_transport_error)?;
        #[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
        {
            cohesix_ninedoor_qemu_evidence_post_prepare();
            if take_ninedoor_qemu_evidence_local_revoke_request() {
                self.namespace_service.revoke();
            }
        }
        Ok(prepared)
    }

    /// Reset per-session state after a console disconnect.
    pub fn reset_session(&mut self) {
        self.attached = false;
        self.session_role = None;
        self.session_ticket = None;
        self.session_scope = None;
        self.binds.clear();
    }

    /// Quietly fence scalar session authority before deferred resource retirement.
    pub(crate) fn fence_session_authority_quiet(&mut self) {
        self.attached = false;
        self.session_role = None;
    }

    /// Move the active ticket allocation into a reboot-lifetime tombstone.
    pub(crate) fn retire_session_ticket_quiet(&mut self) {
        if self.retired_session_ticket.is_none() {
            self.retired_session_ticket = self.session_ticket.take();
        }
    }

    /// Move the active scope allocation into a reboot-lifetime tombstone.
    pub(crate) fn retire_session_scope_quiet(&mut self) {
        if self.retired_session_scope.is_none() {
            self.retired_session_scope = self.session_scope.take();
        }
    }

    /// Move all fixed-capacity binds into a reboot-lifetime tombstone.
    pub(crate) fn retire_session_binds_quiet(&mut self) {
        if self.retired_session_binds.is_empty() {
            core::mem::swap(&mut self.binds, &mut self.retired_session_binds);
        }
    }

    fn with_authority<T>(
        &mut self,
        op: AuthorityOp,
        f: impl FnOnce(&mut Self) -> Result<T, NineDoorBridgeError>,
    ) -> Result<T, NineDoorBridgeError> {
        let token = match self.authority.enter(op) {
            Ok(token) => token,
            Err(AuthorityError::Busy) => return Err(NineDoorBridgeError::Busy),
        };
        let result = f(self);
        self.authority.exit(token);
        result
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
        let prepared = self.prepare_namespace(NamespaceOpcode::Attach, "", role)?;
        let role = prepared.payload();
        let newly_attached = !self.attached;

        // The validated namespace response is the sole fallible application
        // attach gate. Commit local authority before best-effort audit, logger,
        // or tracer diagnostics so those observers cannot veto or roll back a
        // successfully prepared session.
        self.update_session_context(role, ticket);
        self.attached = true;

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
        #[cfg(feature = "kernel")]
        if newly_attached {
            boot_log::notify_bridge_attached();
            // Namespace preparation above is the application attach
            // transaction. The optional logger EP self-test controls only
            // UART mirroring versus EP-only output and cannot veto namespace
            // authority after the target service has replied successfully.
            boot_tracer().advance(BootPhase::EPAttachOk);
        }
        Ok(())
    }

    /// Handle a `tail` request.
    pub fn tail(
        &mut self,
        path: &str,
        audit: &mut dyn AuditSink,
    ) -> Result<(), NineDoorBridgeError> {
        let prepared = self.prepare_namespace(NamespaceOpcode::Tail, path, "")?;
        let path = prepared.path();
        let mut message = HeaplessString::<128>::new();
        if write!(message, "nine-door: tail {path}").is_err() {
            // Truncated audit line is acceptable.
        }
        audit.info(message.as_str());
        Ok(())
    }

    /// Return telemetry lines for a worker ring, if the path targets telemetry.
    pub(crate) fn telemetry_tail(
        &mut self,
        path: &str,
        cursor_offset: u64,
    ) -> Result<Option<TelemetryTail>, NineDoorBridgeError> {
        let mut lines: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        if let Some(meta) = self.telemetry_tail_into(path, cursor_offset, &mut lines)? {
            return Ok(Some(TelemetryTail {
                lines,
                start_offset: meta.start_offset,
                consumed_bytes: meta.consumed_bytes,
            }));
        }
        Ok(None)
    }

    /// Submit one root-validated GPU/PEFT result to the exact READY target
    /// Worker generation. This is the narrow result-to-Worker seam used by the
    /// existing host-ticket namespace; it creates no new transport or verb.
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    pub fn submit_target_worker_operation(
        &mut self,
        control: WorkerControlRecord,
    ) -> Result<TargetWorkerNamespaceSnapshot, NineDoorBridgeError> {
        if !self.is_queen() {
            return Err(NineDoorBridgeError::Permission);
        }
        self.sync_target_worker_projection_for_identity(control.identity)?;
        let snapshot = enqueue_target_worker_operation(control).map_err(worker_target_error)?;
        self.apply_target_worker_projection(snapshot)?;
        Ok(snapshot)
    }

    /// Return the current exact target Worker projection for a public id.
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    pub fn target_worker_snapshot(
        &mut self,
        public_id: &str,
    ) -> Result<TargetWorkerNamespaceSnapshot, NineDoorBridgeError> {
        self.sync_target_worker_projection_by_public_id(public_id)
    }

    fn handle_host_ticket_append(
        &mut self,
        path: &str,
        payload: &str,
    ) -> Result<(), NineDoorBridgeError> {
        let lines = payload
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let mut contains_v2 = false;
        for line in &lines {
            self.host.validate_ticket_line_bytes(line)?;
            let schema = parse_json_string_field(line, "schema")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let accepted = if path.ends_with("/tickets/spec") {
                self.host.accepted_request_schema(schema)
            } else {
                self.host.accepted_result_schema(schema)
            };
            if !accepted {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            contains_v2 |= matches!(
                schema,
                HOST_TICKET_V2_REQUEST_SCHEMA | HOST_TICKET_V2_RESULT_SCHEMA
            );
        }
        if contains_v2 {
            if lines.len() != 1 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if path.ends_with("/tickets/spec") {
                let raw = parse_host_ticket_v2_spec(lines[0], &self.host)?;
                return self.admit_host_ticket_v2(path, raw);
            }
            let result = parse_host_ticket_v2_result(lines[0], &self.host)?;
            return self.apply_host_ticket_v2_result(path, result);
        }

        self.host.validate_append(path, payload)?;
        let owned = lines.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
        self.host.can_append_ticket_lines(path, owned.as_slice())?;
        let snapshot_path = self.host.ticket_snapshot_path(path)?;
        let mirror_snapshot = !path.ends_with("/tickets/spec");
        if mirror_snapshot {
            self.host
                .can_append_ticket_lines(snapshot_path.as_str(), owned.as_slice())?;
        }
        self.host
            .append_ticket_lines_preflighted(path, owned.as_slice())?;
        if mirror_snapshot {
            self.host
                .append_ticket_lines_preflighted(snapshot_path.as_str(), owned.as_slice())?;
        }
        Ok(())
    }

    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    fn admit_host_ticket_v2(
        &mut self,
        path: &str,
        raw: HostTicketV2RawSpec,
    ) -> Result<(), NineDoorBridgeError> {
        self.gpu.withdraw_expired(crate::hal::timebase().now_ms());
        validate_host_ticket_v2_subject(&raw, &self.gpu)?;
        let canonical_raw = serialize_host_ticket(&raw)?;
        let raw_digest = sha256_bytes(canonical_raw.as_bytes());
        let correlation_digest =
            host_ticket_correlation_digest(raw.id.as_str(), raw.idempotency_key.as_str())?;
        if let Some(existing) = self.host.admissions.get(&correlation_digest) {
            return if existing.spec.id == raw.id
                && existing.spec.idempotency_key == raw.idempotency_key
                && existing.raw_digest == raw_digest
            {
                Ok(())
            } else {
                Err(NineDoorBridgeError::InvalidPayload)
            };
        }
        let retirement_digest = host_ticket_admission_retirement_candidate(&self.host.admissions)?;
        let snapshot = self
            .sync_target_worker_projection_by_public_id(raw.receipt_worker_id.as_str())
            .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        let identity = snapshot
            .identity
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        let binding = HostTicketV2WorkerBinding {
            public_id: snapshot
                .public_id()
                .ok_or(NineDoorBridgeError::InvalidPayload)?,
            role: snapshot.role,
            identity,
            ready: snapshot.lifecycle == crate::worker_supervisor::WorkerLifecycleState::Ready,
            ready_sequence: snapshot.ready_sequence,
            current_control_sequence: snapshot.control_sequence,
            last_control_sequence: snapshot.last_control_sequence,
        };
        ensure_host_ticket_worker_available(&self.host.admissions, binding)?;
        let (sequence, next_sequence) =
            next_host_ticket_admission_sequence(self.host.next_admission_sequence)?;
        let admitted = admit_host_ticket_v2_spec(raw, binding, sequence)?;
        let canonical_snapshot = serialize_host_ticket(&admitted)?;
        let snapshot_path = self.host.ticket_snapshot_path(path)?;
        self.host
            .can_append_ticket_lines(path, core::slice::from_ref(&canonical_raw))?;
        self.host.can_append_ticket_lines(
            snapshot_path.as_str(),
            core::slice::from_ref(&canonical_snapshot),
        )?;
        // Active admissions are never retired. At the fixed tracking bound,
        // reserve one slot by removing the oldest terminal admission only
        // after all validation and log preflight have succeeded. Root is the
        // sole owner of this state, so the selected digest cannot change
        // between candidate selection and removal.
        let retired = if let Some(digest) = retirement_digest {
            let admission = self
                .host
                .admissions
                .remove(&digest)
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            Some((digest, admission))
        } else {
            None
        };
        let append_result = self
            .host
            .append_ticket_lines_preflighted(path, core::slice::from_ref(&canonical_raw))
            .and_then(|()| {
                self.host.append_ticket_lines_preflighted(
                    snapshot_path.as_str(),
                    core::slice::from_ref(&canonical_snapshot),
                )
            });
        if let Err(error) = append_result {
            if let Some((digest, admission)) = retired {
                self.host.admissions.insert(digest, admission);
            }
            return Err(error);
        }
        self.host.admissions.insert(
            correlation_digest,
            HostTicketV2Admission {
                spec: admitted,
                raw_digest,
                terminal_result_digest: None,
                terminal_outcome: None,
            },
        );
        self.host.next_admission_sequence = next_sequence;
        Ok(())
    }

    #[cfg(not(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)))]
    fn admit_host_ticket_v2(
        &mut self,
        _path: &str,
        _raw: HostTicketV2RawSpec,
    ) -> Result<(), NineDoorBridgeError> {
        Err(NineDoorBridgeError::InvalidPayload)
    }

    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    fn apply_host_ticket_v2_result(
        &mut self,
        path: &str,
        result: HostTicketV2Result,
    ) -> Result<(), NineDoorBridgeError> {
        let correlation_digest =
            host_ticket_correlation_digest(result.id.as_str(), result.idempotency_key.as_str())?;
        let admission = self
            .host
            .admissions
            .get(&correlation_digest)
            .cloned()
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        validate_result_binding(&result, &admission.spec)?;
        let canonical = serialize_host_ticket(&result)?;
        let result_digest = decode_sha256(&result.result_digest)?;
        let snapshot_path = self.host.ticket_snapshot_path(path)?;
        if let Some(existing_digest) = admission.terminal_result_digest {
            return if existing_digest == result_digest && admission.terminal_outcome.is_some() {
                Ok(())
            } else {
                Err(NineDoorBridgeError::InvalidPayload)
            };
        }
        let terminal_outcome = host_ticket_terminal_outcome(result.state.as_str())?;
        let Some(outcome) = terminal_outcome else {
            self.host
                .can_append_ticket_lines(path, core::slice::from_ref(&canonical))?;
            self.host.can_append_ticket_lines(
                snapshot_path.as_str(),
                core::slice::from_ref(&canonical),
            )?;
            self.host
                .append_ticket_lines_preflighted(path, core::slice::from_ref(&canonical))?;
            self.host.append_ticket_lines_preflighted(
                snapshot_path.as_str(),
                core::slice::from_ref(&canonical),
            )?;
            return Ok(());
        };

        let current_identity = admission_identity(&admission.spec)?;
        let current = target_worker_namespace_snapshot_for_identity(current_identity).ok();
        if let Some(snapshot) = current {
            self.apply_target_worker_projection(snapshot)?;
        }
        let current_binding = current.as_ref().and_then(|snapshot| {
            Some(HostTicketV2WorkerBinding {
                public_id: snapshot.public_id()?,
                role: snapshot.role,
                identity: snapshot.identity?,
                ready: snapshot.lifecycle == crate::worker_supervisor::WorkerLifecycleState::Ready,
                ready_sequence: snapshot.ready_sequence,
                current_control_sequence: snapshot.control_sequence,
                last_control_sequence: snapshot.last_control_sequence,
            })
        });
        let disposition =
            host_ticket_terminal_disposition(outcome, &admission.spec, current_binding)?;
        let worker_control_sequence = match disposition {
            HostTicketV2TerminalDisposition::Submit(_) => {
                Some(next_host_ticket_worker_control_sequence(
                    current_binding.ok_or(NineDoorBridgeError::InvalidPayload)?,
                )?)
            }
            HostTicketV2TerminalDisposition::Stale => None,
        };
        self.host
            .can_append_ticket_lines(path, core::slice::from_ref(&canonical))?;
        self.host
            .can_append_ticket_lines(snapshot_path.as_str(), core::slice::from_ref(&canonical))?;

        if let HostTicketV2TerminalDisposition::Submit(outcome) = disposition {
            let admitted_time_ns = crate::hal::timebase().now_ms().saturating_mul(1_000_000);
            let control = build_host_ticket_worker_control(
                &result,
                outcome,
                admitted_time_ns,
                worker_control_sequence.ok_or(NineDoorBridgeError::InvalidPayload)?,
            )?;
            self.submit_target_worker_operation(control)?;
        }
        self.host
            .append_ticket_lines_preflighted(path, core::slice::from_ref(&canonical))?;
        self.host.append_ticket_lines_preflighted(
            snapshot_path.as_str(),
            core::slice::from_ref(&canonical),
        )?;
        let terminal_outcome = match disposition {
            HostTicketV2TerminalDisposition::Submit(outcome) => outcome,
            HostTicketV2TerminalDisposition::Stale => WorkerOutcome::Stale,
        };
        if let Some(stored) = self.host.admissions.get_mut(&correlation_digest) {
            stored.terminal_result_digest = Some(result_digest);
            stored.terminal_outcome = Some(terminal_outcome);
        }
        Ok(())
    }

    #[cfg(not(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)))]
    fn apply_host_ticket_v2_result(
        &mut self,
        _path: &str,
        _result: HostTicketV2Result,
    ) -> Result<(), NineDoorBridgeError> {
        Err(NineDoorBridgeError::InvalidPayload)
    }

    pub(crate) fn telemetry_tail_into(
        &mut self,
        path: &str,
        cursor_offset: u64,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<Option<TelemetryTailMeta>, NineDoorBridgeError> {
        let Some(worker_id) = parse_worker_telemetry_path(path) else {
            return Ok(None);
        };
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        self.sync_target_worker_projection_by_public_id(worker_id)?;
        let worker = self
            .workers
            .iter()
            .find(|worker| worker.id.as_str() == worker_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        if !worker.target_published {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let read = worker.ring.read_from(cursor_offset, UI_MAX_STREAM_BYTES);
        output.clear();
        lines_from_bytes_into(read.bytes.as_slice(), output)?;
        Ok(Some(TelemetryTailMeta {
            start_offset: read.start_offset,
            consumed_bytes: read.consumed_bytes,
        }))
    }

    /// Fill one bounded TAIL stream from a retained Worker ring or host node.
    pub(crate) fn tail_stream_into(
        &mut self,
        path: &str,
        cursor_offset: u64,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<Option<TelemetryTailMeta>, NineDoorBridgeError> {
        if let Some(meta) = self.telemetry_tail_into(path, cursor_offset, output)? {
            return Ok(Some(meta));
        }
        let Some((text, base_offset)) = self.host.entry_tail_window(path) else {
            return Ok(None);
        };
        bounded_text_tail_into(text, base_offset, cursor_offset, output).map(Some)
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
        let mut output: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        self.ingest_watch_lines_into(now_ms, audit, &mut output)?;
        Ok(output)
    }

    pub fn ingest_watch_lines_into(
        &mut self,
        now_ms: u64,
        audit: &mut dyn AuditSink,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        self.observe.watch_lines_into(now_ms, audit, output)
    }

    /// Handle a log stream request.
    pub fn log_stream(&mut self, audit: &mut dyn AuditSink) -> Result<(), NineDoorBridgeError> {
        self.prepare_namespace(NamespaceOpcode::Log, "", "")?;
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
        let prepared = self.prepare_namespace(NamespaceOpcode::Spawn, "", payload)?;
        let payload = prepared.payload();
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
        let result = self.with_authority(AuthorityOp::QueenCtl, |bridge| {
            bridge.handle_queen_ctl(payload)
        });
        if self.audit.enabled {
            let outcome = ControlOutcome::from_result(&result);
            let role = self.role_label();
            let ticket = String::from(self.ticket_label());
            self.audit
                .record_control(QUEEN_CTL_PATH, payload, outcome, role, ticket.as_str())?;
        }
        result
    }

    /// Handle a kill request.
    pub fn kill(
        &mut self,
        identifier: &str,
        audit: &mut dyn AuditSink,
    ) -> Result<(), NineDoorBridgeError> {
        let prepared = self.prepare_namespace(NamespaceOpcode::Kill, "", identifier)?;
        let identifier = prepared.payload();
        let mut message = HeaplessString::<128>::new();
        if write!(message, "nine-door: kill {identifier}").is_err() {
            // Truncated audit line is acceptable.
        }
        audit.info(message.as_str());
        let payload = format!("{{\"kill\":\"{}\"}}", escape_json_string(identifier));
        let result = self.with_authority(AuthorityOp::QueenCtl, |bridge| {
            bridge.remove_worker(identifier)
        });
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
    pub fn echo(&mut self, path: &str, payload: &str) -> Result<EchoOutcome, NineDoorBridgeError> {
        self.gpu.withdraw_expired(crate::hal::timebase().now_ms());
        let prepared = self.prepare_namespace(NamespaceOpcode::Echo, path, payload)?;
        let path = prepared.path();
        let payload = prepared.payload();
        let segments = split_path_segments(path);
        if path == LOG_PATH {
            log_buffer::append_user_line(payload);
            log_buffer::append_log_line(payload);
            return Ok(EchoOutcome::Appended);
        }
        if self.audit.enabled {
            if path == AUDIT_JOURNAL_PATH {
                self.audit.append_manual_journal(payload)?;
                return Ok(EchoOutcome::Appended);
            }
            if path == AUDIT_DECISIONS_PATH || path == AUDIT_EXPORT_PATH {
                return Err(NineDoorBridgeError::Permission);
            }
        }
        if self.replay.enabled {
            if path == REPLAY_CTL_PATH {
                self.replay.handle_ctl(payload, &mut self.audit)?;
                return Ok(EchoOutcome::Appended);
            }
            if path == REPLAY_STATUS_PATH {
                return Err(NineDoorBridgeError::Permission);
            }
        }
        if self.policy.enabled {
            if path == POLICY_CTL_PATH {
                self.with_authority(AuthorityOp::PolicyCtl, |bridge| {
                    bridge.policy.append_policy_ctl(payload)?;
                    Ok(())
                })?;
                return Ok(EchoOutcome::Appended);
            }
            if path == ACTIONS_QUEUE_PATH {
                self.with_authority(AuthorityOp::ActionsQueue, |bridge| {
                    let role = bridge.role_label();
                    let ticket = String::from(bridge.ticket_label());
                    let before = bridge.policy.actions.len();
                    bridge
                        .policy
                        .append_action_queue(payload, role, ticket.as_str())?;
                    if bridge.audit.enabled {
                        for action in bridge.policy.actions.iter().skip(before) {
                            bridge
                                .audit
                                .record_decision_action(action, role, ticket.as_str())?;
                        }
                    }
                    Ok(())
                })?;
                return Ok(EchoOutcome::Appended);
            }
        }
        if path == QUEEN_SCHEDULE_CTL_PATH {
            self.with_authority(AuthorityOp::ScheduleCtl, |bridge| {
                if !bridge.schedule.enabled() {
                    return Err(NineDoorBridgeError::InvalidPath);
                }
                if !bridge.is_queen() {
                    if bridge.audit.enabled {
                        let role = bridge.role_label();
                        let ticket = String::from(bridge.ticket_label());
                        bridge.audit.record_control(
                            path,
                            payload,
                            ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                            role,
                            ticket.as_str(),
                        )?;
                    }
                    return Err(NineDoorBridgeError::Permission);
                }
                let role = bridge.role_label();
                let ticket = String::from(bridge.ticket_label());
                let decision = bridge.apply_policy_gate(path)?;
                match decision {
                    PolicyGateDecision::Denied(_) => {
                        if bridge.audit.enabled {
                            bridge.audit.record_control(
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
                let result = bridge.schedule.append_ctl(payload);
                if bridge.audit.enabled {
                    let outcome = ControlOutcome::from_result(&result);
                    bridge
                        .audit
                        .record_control(path, payload, outcome, role, ticket.as_str())?;
                }
                result
            })?;
            return Ok(EchoOutcome::Appended);
        }
        if path == QUEEN_LEASE_CTL_PATH {
            self.with_authority(AuthorityOp::LeaseCtl, |bridge| {
                if !bridge.lease.enabled() {
                    return Err(NineDoorBridgeError::InvalidPath);
                }
                if !bridge.is_queen() {
                    if bridge.audit.enabled {
                        let role = bridge.role_label();
                        let ticket = String::from(bridge.ticket_label());
                        bridge.audit.record_control(
                            path,
                            payload,
                            ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                            role,
                            ticket.as_str(),
                        )?;
                    }
                    return Err(NineDoorBridgeError::Permission);
                }
                let role = bridge.role_label();
                let ticket = String::from(bridge.ticket_label());
                let decision = bridge.apply_policy_gate(path)?;
                match decision {
                    PolicyGateDecision::Denied(_) => {
                        if bridge.audit.enabled {
                            bridge.audit.record_control(
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
                let result = bridge.lease.append_ctl(payload);
                if bridge.audit.enabled {
                    let outcome = ControlOutcome::from_result(&result);
                    bridge
                        .audit
                        .record_control(path, payload, outcome, role, ticket.as_str())?;
                }
                result
            })?;
            return Ok(EchoOutcome::Appended);
        }
        if path == QUEEN_EXPORT_CTL_PATH {
            self.with_authority(AuthorityOp::ExportCtl, |bridge| {
                if !bridge.export.enabled() {
                    return Err(NineDoorBridgeError::InvalidPath);
                }
                if !bridge.is_queen() {
                    if bridge.audit.enabled {
                        let role = bridge.role_label();
                        let ticket = String::from(bridge.ticket_label());
                        bridge.audit.record_control(
                            path,
                            payload,
                            ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                            role,
                            ticket.as_str(),
                        )?;
                    }
                    return Err(NineDoorBridgeError::Permission);
                }
                let role = bridge.role_label();
                let ticket = String::from(bridge.ticket_label());
                let decision = bridge.apply_policy_gate(path)?;
                match decision {
                    PolicyGateDecision::Denied(_) => {
                        if bridge.audit.enabled {
                            bridge.audit.record_control(
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
                let result = bridge.export.append_ctl(payload);
                if bridge.audit.enabled {
                    let outcome = ControlOutcome::from_result(&result);
                    bridge
                        .audit
                        .record_control(path, payload, outcome, role, ticket.as_str())?;
                }
                result
            })?;
            return Ok(EchoOutcome::Appended);
        }
        if path == QUEEN_LIFECYCLE_CTL_PATH {
            self.with_authority(AuthorityOp::LifecycleCtl, |bridge| {
                if !bridge.is_queen() {
                    if bridge.audit.enabled {
                        let role = bridge.role_label();
                        let ticket = String::from(bridge.ticket_label());
                        bridge.audit.record_control(
                            path,
                            payload,
                            ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                            role,
                            ticket.as_str(),
                        )?;
                    }
                    return Err(NineDoorBridgeError::Permission);
                }
                let role = bridge.role_label();
                let ticket = String::from(bridge.ticket_label());
                let decision = bridge.apply_policy_gate(path)?;
                match decision {
                    PolicyGateDecision::Denied(_) => {
                        if bridge.audit.enabled {
                            bridge.audit.record_control(
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
                let result = bridge.handle_lifecycle_ctl(payload);
                if bridge.audit.enabled {
                    let outcome = ControlOutcome::from_result(&result);
                    bridge
                        .audit
                        .record_control(path, payload, outcome, role, ticket.as_str())?;
                }
                result
            })?;
            return Ok(EchoOutcome::Appended);
        }
        if path == QUEEN_CTL_PATH {
            self.with_authority(AuthorityOp::QueenCtl, |bridge| {
                let role = bridge.role_label();
                let ticket = String::from(bridge.ticket_label());
                let decision = bridge.apply_policy_gate(path)?;
                match decision {
                    PolicyGateDecision::Denied(_) => {
                        if bridge.audit.enabled {
                            bridge.audit.record_control(
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
                let result = bridge.handle_queen_ctl(payload);
                if bridge.audit.enabled {
                    let outcome = ControlOutcome::from_result(&result);
                    bridge
                        .audit
                        .record_control(path, payload, outcome, role, ticket.as_str())?;
                }
                result
            })?;
            return Ok(EchoOutcome::Appended);
        }
        if let Some(device_id) = telemetry_ingest_ctl_device(path) {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            self.ensure_lifecycle_gate(lifecycle::GATE_TELEMETRY_INGEST)?;
            if !self.is_queen() {
                return Err(NineDoorBridgeError::Permission);
            }
            parse_telemetry_ctl(payload)?;
            let seg_id = self.telemetry_ingest.create_segment(device_id)?;
            return Ok(EchoOutcome::TelemetrySegmentCreated { seg_id });
        }
        if let Some((device_id, seg_id)) = telemetry_ingest_segment_path(path) {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            self.ensure_lifecycle_gate(lifecycle::GATE_TELEMETRY_INGEST)?;
            if !self.is_queen() {
                return Err(NineDoorBridgeError::Permission);
            }
            self.telemetry_ingest
                .append_record(device_id, seg_id, payload)?;
            return Ok(EchoOutcome::Appended);
        }
        if telemetry_ingest_latest_path(path).is_some() {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return Err(NineDoorBridgeError::Permission);
        }
        if segments.as_slice() == ["gpu", "bridge", "ctl"] {
            if !self.gpu.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if !self.is_queen() {
                return Err(NineDoorBridgeError::Permission);
            }
            self.ensure_lifecycle_gate(lifecycle::GATE_HOST_PUBLISH)?;
            let trimmed = trim_payload(payload.as_bytes());
            let text =
                core::str::from_utf8(trimmed).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            self.gpu.handle_bridge_payload(text)?;
            return Ok(EchoOutcome::Appended);
        }
        if segments.as_slice() == ["gpu", "models", "active"] {
            if !self.gpu.enabled() || !self.gpu.models_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            // Active state is installed only by a validated, receipt-bound host snapshot.
            return Err(NineDoorBridgeError::Permission);
        }
        if let ["gpu", gpu_id, leaf] = segments.as_slice() {
            if !self.gpu.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if !self.is_queen() && !matches!(self.session_role, Some(SessionRoleLabel::WorkerGpu)) {
                return Err(NineDoorBridgeError::Permission);
            }
            let ctl_max = self.gpu.ctl_max_bytes;
            let lease_max = self.gpu.lease_max_bytes;
            let status_max = self.gpu.status_max_bytes;
            let entry = self
                .gpu
                .entry_mut(gpu_id)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            match *leaf {
                "ctl" => append_log_bytes(&mut entry.ctl_log, payload, ctl_max)?,
                "lease" => {
                    validate_json_envelope(payload)?;
                    append_log_bytes(&mut entry.lease_log, payload, lease_max)?
                }
                "status" => {
                    validate_json_envelope(payload)?;
                    append_log_bytes(&mut entry.status_log, payload, status_max)?
                }
                _ => return Err(NineDoorBridgeError::InvalidPath),
            }
            return Ok(EchoOutcome::Appended);
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
            if !self.host.writable(path) {
                self.log_host_write(path, Some(control), HostWriteOutcome::Denied, None);
                return Err(NineDoorBridgeError::Permission);
            }
            self.ensure_lifecycle_gate(lifecycle::GATE_HOST_PUBLISH)?;
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
            if self.host.is_ticket_write_path(path) {
                self.handle_host_ticket_append(path, payload)?;
            } else {
                self.host.validate_append(path, payload)?;
                self.host.update_value(path, payload);
            }
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
            return Ok(EchoOutcome::Appended);
        }
        if self.host.entry_value(path).is_some() {
            if !self.is_queen() {
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
            if !self.host.writable(path) {
                self.log_host_write(path, None, HostWriteOutcome::Denied, None);
                return Err(NineDoorBridgeError::Permission);
            }
            self.ensure_lifecycle_gate(lifecycle::GATE_HOST_PUBLISH)?;
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
            if self.host.is_ticket_write_path(path) {
                self.handle_host_ticket_append(path, payload)?;
            } else {
                self.host.validate_append(path, payload)?;
                self.host.update_value(path, payload);
            }
            self.log_host_write(path, None, HostWriteOutcome::Allowed, Some(payload.len()));
            if self.audit.enabled {
                self.audit.record_control(
                    path,
                    payload,
                    ControlOutcome::ok(),
                    role,
                    ticket.as_str(),
                )?;
            }
            return Ok(EchoOutcome::Appended);
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
                return Ok(EchoOutcome::Appended);
            }
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let resolved = self.resolve_bound_path(path);
        let resolved_path = resolved.as_deref().unwrap_or(path);
        if let Some(outcome) =
            self.cas
                .append_path(resolved_path, payload.as_bytes(), self.is_queen())?
        {
            let () = outcome;
            return Ok(EchoOutcome::Appended);
        }
        if let Some(worker_id) = parse_worker_telemetry_path(resolved_path) {
            self.append_worker_telemetry(worker_id, payload.as_bytes())?;
            return Ok(EchoOutcome::Appended);
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
        let mut output = HeaplessVec::new();
        self.cat_into(path, &mut output)?;
        Ok(output)
    }

    /// Read file contents into a caller-provided buffer to avoid stack-heavy temporaries.
    pub fn cat_into(
        &mut self,
        path: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        self.gpu.withdraw_expired(crate::hal::timebase().now_ms());
        let prepared = self.prepare_namespace(NamespaceOpcode::Cat, path, "")?;
        let path = prepared.path();
        output.clear();
        let segments = split_path_segments(path);
        if path == LOG_PATH {
            log_buffer::snapshot_lines_into(output);
            return Ok(());
        }
        if self.audit.enabled {
            if path == AUDIT_JOURNAL_PATH {
                return lines_from_bytes_into(&self.audit.journal_snapshot(), output);
            }
            if path == AUDIT_DECISIONS_PATH {
                return lines_from_bytes_into(&self.audit.decisions_snapshot(), output);
            }
            if path == AUDIT_EXPORT_PATH {
                return lines_from_bytes_into(&self.audit.export_snapshot(), output);
            }
        }
        if self.replay.enabled {
            if path == REPLAY_CTL_PATH {
                return lines_from_bytes_into(self.replay.ctl_log(), output);
            }
            if path == REPLAY_STATUS_PATH {
                return lines_from_bytes_into(self.replay.status(), output);
            }
        }
        if self.policy.enabled {
            if self.ui.policy_preflight.req && path == POLICY_PREFLIGHT_REQ_PATH {
                let payload = self.policy.preflight_req_text()?;
                return lines_from_bytes_into(payload.as_slice(), output);
            }
            if self.ui.policy_preflight.req && path == POLICY_PREFLIGHT_REQ_CBOR_PATH {
                let payload = self.policy.preflight_req_cbor()?;
                return cas_lines_from_bytes_into(payload.as_slice(), output);
            }
            if self.ui.policy_preflight.diff && path == POLICY_PREFLIGHT_DIFF_PATH {
                let payload = self.policy.preflight_diff_text()?;
                return lines_from_bytes_into(payload.as_slice(), output);
            }
            if self.ui.policy_preflight.diff && path == POLICY_PREFLIGHT_DIFF_CBOR_PATH {
                let payload = self.policy.preflight_diff_cbor()?;
                return cas_lines_from_bytes_into(payload.as_slice(), output);
            }
            if path == POLICY_RULES_PATH {
                return script_lines_into(self.policy.rules_json(), output);
            }
            if path == POLICY_CTL_PATH {
                return lines_from_bytes_into(self.policy.ctl_log(), output);
            }
            if path == ACTIONS_QUEUE_PATH {
                return lines_from_bytes_into(self.policy.queue_log(), output);
            }
            if let Some(action_id) = parse_action_status_path(path) {
                return self.action_status_lines_into(action_id, output);
            }
        }
        if self.schedule.enabled() && path == QUEEN_SCHEDULE_CTL_PATH {
            return lines_from_bytes_into(self.schedule.ctl_log(), output);
        }
        if self.lease.enabled() && path == QUEEN_LEASE_CTL_PATH {
            return lines_from_bytes_into(self.lease.ctl_log(), output);
        }
        if self.export.enabled() && path == QUEEN_EXPORT_CTL_PATH {
            return lines_from_bytes_into(self.export.ctl_log(), output);
        }
        if self.export.enabled() && self.gpu.qemu_lora_export_ready() {
            match segments.as_slice() {
                ["queen", "export", "lora_jobs", QEMU_LORA_EXPORT_JOB_ID, "telemetry.cbor"] => {
                    return lines_from_bytes_into(QEMU_LORA_EXPORT_TELEMETRY, output);
                }
                ["queen", "export", "lora_jobs", QEMU_LORA_EXPORT_JOB_ID, "base_model.ref"] => {
                    return lines_from_bytes_into(QEMU_LORA_EXPORT_BASE_MODEL, output);
                }
                ["queen", "export", "lora_jobs", QEMU_LORA_EXPORT_JOB_ID, "policy.toml"] => {
                    return lines_from_bytes_into(QEMU_LORA_EXPORT_POLICY, output);
                }
                _ => {}
            }
        }
        if self.schedule.proc_enabled() {
            if path == PROC_SCHEDULE_SUMMARY_PATH {
                return self.schedule.summary_lines_into(output);
            }
            if path == PROC_SCHEDULE_QUEUE_PATH {
                return self.schedule.queue_lines_into(output);
            }
            #[cfg(feature = "release-qemu")]
            if path == PROC_SCHEDULE_QEMU_FLIGHT_PATH {
                crate::qemu_flight_recorder::snapshot_lines_into(output);
                return Ok(());
            }
        }
        if self.lease.proc_enabled() {
            if path == PROC_LEASE_SUMMARY_PATH {
                return self.lease.summary_lines_into(output);
            }
            if path == PROC_LEASE_ACTIVE_PATH {
                return self.lease.active_lines_into(output);
            }
            if let Some(id) = parse_proc_lease_by_id_path(path)? {
                if !self.lease.proc_active_enabled() {
                    return Err(NineDoorBridgeError::InvalidPath);
                }
                return self.lease.active_line_into(id, output);
            }
            if path == PROC_LEASE_PREEMPTIONS_PATH {
                return self.lease.preemptions_lines_into(output);
            }
        }
        if segments.as_slice() == ["gpu", "bridge", "ctl"] {
            if !self.gpu.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return lines_from_bytes_into(self.gpu.bridge.ctl_log.as_slice(), output);
        }
        if segments.as_slice() == ["gpu", "bridge", "status"] {
            if !self.gpu.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return lines_from_bytes_into(self.gpu.bridge.status.as_slice(), output);
        }
        if segments.as_slice() == ["gpu", "models", "active"] {
            if !self.gpu.enabled() || !self.gpu.models_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return lines_from_bytes_into(self.gpu.models_active_log.as_slice(), output);
        }
        if let ["gpu", "models", "available", model_id, "manifest.toml"] = segments.as_slice() {
            if !self.gpu.enabled() || !self.gpu.models_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if let Some(entry) = self
                .gpu
                .models
                .iter()
                .find(|entry| entry.model_id == *model_id)
            {
                return lines_from_text_into(entry.manifest_toml.as_str(), output);
            }
            return Err(NineDoorBridgeError::InvalidPath);
        }
        if segments.as_slice() == ["gpu", "telemetry", "schema.json"] {
            if !self.gpu.enabled() || !self.gpu.telemetry_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return lines_from_bytes_into(self.gpu.telemetry_schema.as_slice(), output);
        }
        if let ["gpu", gpu_id, leaf] = segments.as_slice() {
            if !self.gpu.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            let entry = self
                .gpu
                .entry(gpu_id)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            return match *leaf {
                "info" => lines_from_text_into(entry.info_payload.as_str(), output),
                "ctl" => lines_from_bytes_into(entry.ctl_log.as_slice(), output),
                "lease" => lines_from_bytes_into(entry.lease_log.as_slice(), output),
                "status" => lines_from_bytes_into(entry.status_log.as_slice(), output),
                _ => Err(NineDoorBridgeError::InvalidPath),
            };
        }
        if let Some((device_id, seg_id)) = telemetry_ingest_segment_path(path) {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            let device = self
                .telemetry_ingest
                .device(device_id)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            let segment = device
                .segments
                .iter()
                .find(|segment| segment.id == seg_id)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            return lines_from_bytes_into(segment.data.as_slice(), output);
        }
        if let Some(device_id) = telemetry_ingest_latest_path(path) {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            let device = self
                .telemetry_ingest
                .device(device_id)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            if let Some(latest) = device.latest.as_deref() {
                return lines_from_text_into(latest, output);
            }
            return Ok(());
        }
        if let Some(kind) = self.sidecars.kind_for_path(segments.as_slice()) {
            if !self.sidecar_allowed(kind, segments.as_slice(), SidecarAccess::Read) {
                self.log_sidecar_denial(kind);
                return Err(NineDoorBridgeError::Permission);
            }
            if let Some(data) = self.sidecars.read(segments.as_slice()) {
                return lines_from_bytes_into(&data, output);
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
                return cas_lines_from_bytes_into(payloads.cbor.as_slice(), output);
            }
            return lines_from_bytes_into(payloads.text.as_slice(), output);
        }
        if let Some(bytes) = self.cas.read_path(path, self.is_queen())? {
            return cas_lines_from_bytes_into(&bytes, output);
        }
        if self.host.is_ticket_retention_path(path) {
            return self.host.retention_lines_into(output);
        }
        if let Some(correlation_digest) = parse_host_ticket_current_path(path)? {
            #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
            {
                let admission = self
                    .host
                    .admissions
                    .get(&correlation_digest)
                    .ok_or(NineDoorBridgeError::InvalidPath)?;
                let current = target_worker_namespace_snapshot_for_identity(admission_identity(
                    &admission.spec,
                )?)
                .ok();
                output
                    .push(host_ticket_current_line(admission, current.as_ref())?)
                    .map_err(|_| NineDoorBridgeError::BufferFull)?;
                return Ok(());
            }
            #[cfg(not(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)))]
            {
                let _ = correlation_digest;
                return Err(NineDoorBridgeError::InvalidPath);
            }
        }
        if let Some(value) = self.host.entry_value(path) {
            return cat_lines_from_text_into(value, output);
        }
        if path == PROC_BOOT_PATH {
            return boot_lines_into(output);
        }
        if path == PROC_TESTS_QUICK_PATH {
            return script_lines_into(SELFTEST_QUICK_SCRIPT, output);
        }
        if path == PROC_TESTS_FULL_PATH {
            return script_lines_into(SELFTEST_FULL_SCRIPT, output);
        }
        if path == PROC_TESTS_NEGATIVE_PATH {
            return script_lines_into(SELFTEST_NEGATIVE_SCRIPT, output);
        }
        if path == PROC_TESTS_SMP_PATH {
            return script_lines_into(SELFTEST_SMP_SCRIPT, output);
        }
        if matches!(
            path,
            PROC_LIFECYCLE_STATE_PATH | PROC_LIFECYCLE_REASON_PATH | PROC_LIFECYCLE_SINCE_PATH
        ) {
            let snapshot = lifecycle::snapshot();
            let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
            match path {
                PROC_LIFECYCLE_STATE_PATH => {
                    let _ = write!(line, "state={}", lifecycle::state_label(snapshot.state));
                }
                PROC_LIFECYCLE_REASON_PATH => {
                    let _ = write!(line, "reason={}", snapshot.reason.as_str());
                }
                PROC_LIFECYCLE_SINCE_PATH => {
                    let _ = write!(line, "since_ms={}", snapshot.since_ms);
                }
                _ => {}
            }
            return lines_from_text_into(line.as_str(), output);
        }
        if path == PROC_9P_SESSION_ACTIVE_PATH {
            if !self.observe.proc_9p_session_enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
            let active = if self.attached { 1 } else { 0 };
            let _ = write!(line, "active={} draining=0", active);
            return lines_from_text_into(line.as_str(), output);
        }
        if let Some(result) = self.observe.root_lines_into(path, output) {
            return result;
        }
        if let Some(result) = self.observe.pressure_lines_into(path, output) {
            return result;
        }
        if let Some(result) = self.observe.ingest_lines_into(path, output) {
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
        let mut output = HeaplessVec::new();
        self.list_into(path, &mut output)?;
        Ok(output)
    }

    /// List directory entries into a caller-provided buffer to avoid stack-heavy temporaries.
    pub fn list_into(
        &mut self,
        path: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        self.sync_target_worker_projections()?;
        self.gpu.withdraw_expired(crate::hal::timebase().now_ms());
        let prepared = self.prepare_namespace(NamespaceOpcode::List, path, "")?;
        let path = prepared.path();
        output.clear();
        let sharding = generated::sharding_config();
        let segments = split_path_segments(path);
        if path == "/worker" {
            if sharding.enabled && !legacy_worker_alias_enabled(sharding) {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            self.list_workers_into(output)?;
            return Ok(());
        }
        if path == "/shard" {
            if !sharding.enabled {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            self.list_worker_shards_into(output)?;
            return Ok(());
        }
        if let Some((label, worker_root)) = parse_shard_worker_root(path) {
            if !sharding.enabled || !shard_label_known(label) {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if worker_root {
                self.list_workers_for_shard_into(label, output)?;
                return Ok(());
            }
            list_from_slice_into(&["worker"], output)?;
            return Ok(());
        }
        if path == "/" {
            for entry in ["gpu", "kmesg", "log", "proc", "queen", "trace"] {
                push_list_entry(output, entry)?;
            }
            if self.cas.enabled() {
                push_list_entry(output, "updates")?;
                if self.cas.models_enabled() {
                    push_list_entry(output, "models")?;
                }
            }
            if sharding.enabled {
                push_list_entry(output, "shard")?;
            }
            if !sharding.enabled || legacy_worker_alias_enabled(sharding) {
                push_list_entry(output, "worker")?;
            }
            if self.host.enabled {
                push_list_entry(output, self.host.mount_label())?;
            }
            self.sidecars.push_root_entries(output)?;
            if self.policy.enabled {
                push_list_entry(output, "policy")?;
                push_list_entry(output, "actions")?;
            }
            if self.audit.enabled {
                push_list_entry(output, "audit")?;
            }
            if self.replay.enabled {
                push_list_entry(output, "replay")?;
            }
            return Ok(());
        }
        if path == "/log" {
            list_from_slice_into(&["queen.log"], output)?;
            return Ok(());
        }
        if path == "/proc" {
            push_list_entry(output, "boot")?;
            push_list_entry(output, "tests")?;
            push_list_entry(output, "lifecycle")?;
            if self.observe.proc_9p_session_enabled() {
                push_list_entry(output, "9p")?;
            }
            if self.observe.proc_ingest_enabled() {
                push_list_entry(output, "ingest")?;
            }
            if self.observe.proc_root_enabled() {
                push_list_entry(output, "root")?;
            }
            if self.observe.proc_pressure_enabled() {
                push_list_entry(output, "pressure")?;
            }
            if self.schedule.proc_enabled() {
                push_list_entry(output, "schedule")?;
            }
            if self.lease.proc_enabled() {
                push_list_entry(output, "lease")?;
            }
            return Ok(());
        }
        if path == PROC_9P_ROOT_PATH {
            if !self.observe.proc_9p_session_enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["session"], output)?;
            return Ok(());
        }
        if path == PROC_9P_SESSION_ROOT_PATH {
            if !self.observe.proc_9p_session_enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["active"], output)?;
            return Ok(());
        }
        if path == PROC_LIFECYCLE_ROOT_PATH {
            list_from_slice_into(&["state", "reason", "since"], output)?;
            return Ok(());
        }
        if path == PROC_ROOT_ROOT_PATH {
            self.observe.list_root_into(output)?;
            return Ok(());
        }
        if path == "/proc/tests" {
            list_from_slice_into(
                &[
                    "selftest_quick.coh",
                    "selftest_full.coh",
                    "selftest_negative.coh",
                    "selftest_smp.coh",
                ],
                output,
            )?;
            return Ok(());
        }
        if path == PROC_INGEST_ROOT_PATH {
            self.observe.list_ingest_into(output)?;
            return Ok(());
        }
        if path == PROC_PRESSURE_ROOT_PATH {
            self.observe.list_pressure_into(output)?;
            return Ok(());
        }
        if path == PROC_SCHEDULE_ROOT_PATH {
            if !self.schedule.proc_enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            self.schedule.list_proc_into(output)?;
            #[cfg(feature = "release-qemu")]
            push_list_entry(output, "qemu-flight")?;
            return Ok(());
        }
        if path == PROC_LEASE_ROOT_PATH {
            if !self.lease.proc_enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            self.lease.list_proc_into(output)?;
            return Ok(());
        }
        if path == PROC_LEASE_BY_ID_PATH {
            if !self.lease.proc_active_enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            self.lease.list_active_ids_into(output)?;
            return Ok(());
        }
        if path == "/queen" {
            push_list_entry(output, "ctl")?;
            push_list_entry(output, "lifecycle")?;
            if self.schedule.enabled() {
                push_list_entry(output, "schedule")?;
            }
            if self.lease.enabled() {
                push_list_entry(output, "lease")?;
            }
            if self.export.enabled() {
                push_list_entry(output, "export")?;
            }
            if self.telemetry_ingest.enabled() {
                push_list_entry(output, "telemetry")?;
            }
            return Ok(());
        }
        if path == QUEEN_LIFECYCLE_ROOT_PATH {
            list_from_slice_into(&["ctl"], output)?;
            return Ok(());
        }
        if path == QUEEN_SCHEDULE_ROOT_PATH {
            if !self.schedule.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["ctl"], output)?;
            return Ok(());
        }
        if path == QUEEN_LEASE_ROOT_PATH {
            if !self.lease.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["ctl"], output)?;
            return Ok(());
        }
        if path == QUEEN_EXPORT_ROOT_PATH {
            if !self.export.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["ctl", "lora_jobs"], output)?;
            return Ok(());
        }
        if path == "/queen/export/lora_jobs" {
            if !self.export.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if self.gpu.qemu_lora_export_ready() {
                push_list_entry(output, QEMU_LORA_EXPORT_JOB_ID)?;
            }
            return Ok(());
        }
        if segments.as_slice() == ["queen", "export", "lora_jobs", QEMU_LORA_EXPORT_JOB_ID] {
            if !self.export.enabled() || !self.gpu.qemu_lora_export_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["telemetry.cbor", "base_model.ref", "policy.toml"], output)?;
            return Ok(());
        }
        if path == "/queen/telemetry" {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            for device_id in self.telemetry_ingest.devices.keys() {
                push_list_entry(output, device_id)?;
            }
            return Ok(());
        }
        if let Some(device_id) = telemetry_ingest_device_root(path) {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if self.telemetry_ingest.device(device_id).is_none() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["ctl", "seg", "latest"], output)?;
            return Ok(());
        }
        if let Some(device_id) = telemetry_ingest_seg_dir(path) {
            if !self.telemetry_ingest.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            let device = self
                .telemetry_ingest
                .device(device_id)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            for segment in device.segments.iter() {
                push_list_entry(output, segment.id.as_str())?;
            }
            return Ok(());
        }
        if path == "/trace" {
            list_from_slice_into(&["ctl", "events"], output)?;
            return Ok(());
        }
        if segments.as_slice() == ["gpu"] {
            if !self.gpu.enabled() {
                return Ok(());
            }
            push_list_entry(output, "bridge")?;
            if self.gpu.models_ready() {
                push_list_entry(output, "models")?;
            }
            if self.gpu.telemetry_ready() {
                push_list_entry(output, "telemetry")?;
            }
            for entry in self.gpu.entries.iter() {
                push_list_entry(output, entry.id.as_str())?;
            }
            return Ok(());
        }
        if segments.as_slice() == ["gpu", "bridge"] {
            if !self.gpu.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["ctl", "status"], output)?;
            return Ok(());
        }
        if segments.as_slice() == ["gpu", "models"] {
            if !self.gpu.enabled() || !self.gpu.models_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["available", "active"], output)?;
            return Ok(());
        }
        if segments.as_slice() == ["gpu", "models", "available"] {
            if !self.gpu.enabled() || !self.gpu.models_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            for model in &self.gpu.models {
                push_list_entry(output, model.model_id.as_str())?;
            }
            return Ok(());
        }
        if let ["gpu", "models", "available", model_id] = segments.as_slice() {
            if !self.gpu.enabled() || !self.gpu.models_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if self
                .gpu
                .models
                .iter()
                .any(|entry| entry.model_id == *model_id)
            {
                list_from_slice_into(&["manifest.toml"], output)?;
                return Ok(());
            }
            return Err(NineDoorBridgeError::InvalidPath);
        }
        if segments.as_slice() == ["gpu", "telemetry"] {
            if !self.gpu.enabled() || !self.gpu.telemetry_ready() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["schema.json"], output)?;
            return Ok(());
        }
        if let ["gpu", gpu_id] = segments.as_slice() {
            if !self.gpu.enabled() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            if self.gpu.entry(gpu_id).is_none() {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            list_from_slice_into(&["info", "ctl", "lease", "status"], output)?;
            return Ok(());
        }
        if path == "/worker" {
            if sharding.enabled && !legacy_worker_alias_enabled(sharding) {
                return Err(NineDoorBridgeError::InvalidPath);
            }
            return Ok(());
        }
        if self.policy.enabled {
            if path == POLICY_ROOT_PATH {
                push_list_entry(output, "ctl")?;
                push_list_entry(output, "rules")?;
                if self.ui.policy_preflight.req || self.ui.policy_preflight.diff {
                    push_list_entry(output, "preflight")?;
                }
                return Ok(());
            }
            if path == POLICY_PREFLIGHT_ROOT_PATH {
                if !self.ui.policy_preflight.req && !self.ui.policy_preflight.diff {
                    return Err(NineDoorBridgeError::InvalidPath);
                }
                if self.ui.policy_preflight.req {
                    push_list_entry(output, "req")?;
                    push_list_entry(output, "req.cbor")?;
                }
                if self.ui.policy_preflight.diff {
                    push_list_entry(output, "diff")?;
                    push_list_entry(output, "diff.cbor")?;
                }
                return Ok(());
            }
            if path == ACTIONS_ROOT_PATH {
                list_from_slice_into(&["queue"], output)?;
                return Ok(());
            }
        }
        if self.audit.enabled && path == AUDIT_ROOT_PATH {
            list_from_slice_into(&["journal", "decisions", "export"], output)?;
            return Ok(());
        }
        if self.replay.enabled && path == REPLAY_ROOT_PATH {
            list_from_slice_into(&["ctl", "status"], output)?;
            return Ok(());
        }
        if let Some(kind) = self.sidecars.kind_for_path(segments.as_slice()) {
            if !self.sidecar_allowed(kind, segments.as_slice(), SidecarAccess::List) {
                self.log_sidecar_denial(kind);
                return Err(NineDoorBridgeError::Permission);
            }
            if let Some(result) = self.sidecars.list_into(segments.as_slice(), output) {
                return result;
            }
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let resolved = self.resolve_bound_path(path);
        let path = resolved.as_deref().unwrap_or(path);
        if self.cas.list_path_into(
            path,
            self.is_queen(),
            self.ui.updates.manifest,
            self.ui.updates.status,
            output,
        )? {
            return Ok(());
        }
        if let Some(result) = self.host.list_into(path, output) {
            return result;
        }
        Err(NineDoorBridgeError::InvalidPath)
    }

    fn list_workers_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        // Keep directory listings bounded while surfacing the most recently
        // spawned workers at high scale.
        let mut recent: HeaplessVec<&str, MAX_STREAM_LINES> = HeaplessVec::new();
        for worker in self.workers.iter().rev() {
            #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
            if !worker.target_published {
                continue;
            }
            if recent.push(worker.id.as_str()).is_err() {
                break;
            }
        }
        for worker_id in recent.iter().rev() {
            push_list_entry(output, worker_id)?;
        }
        Ok(())
    }

    fn list_worker_shards_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let sharding = generated::sharding_config();
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        {
            // Target discovery reflects only the compiler-bounded current
            // projections. A queued generation remains unpublished until its
            // durable READY record is accepted.
            for snapshot in target_worker_namespace_snapshots() {
                if snapshot.ready_sequence == 0 {
                    continue;
                }
                let public_id = snapshot
                    .public_id()
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                let label = worker_shard_label(public_id, sharding);
                if !output.iter().any(|entry| entry.as_str() == label.as_str()) {
                    push_list_entry(output, label.as_str())?;
                }
            }
            Ok(())
        }

        #[cfg(not(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)))]
        {
            // The host model may contain more active shards than one bounded
            // directory reply. Surface the most recent distinct active shards,
            // matching the existing bounded recent-Worker listing contract.
            let mut recent: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
                HeaplessVec::new();
            for worker in self.workers.iter().rev() {
                let label = worker_shard_label(worker.id.as_str(), sharding);
                if recent.iter().any(|entry| entry.as_str() == label.as_str()) {
                    continue;
                }
                let mut entry = HeaplessString::new();
                entry
                    .push_str(label.as_str())
                    .map_err(|_| NineDoorBridgeError::BufferFull)?;
                if recent.push(entry).is_err() {
                    break;
                }
            }
            for label in recent.iter().rev() {
                push_list_entry(output, label.as_str())?;
            }
            Ok(())
        }
    }

    fn list_workers_for_shard_into(
        &self,
        label: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let sharding = generated::sharding_config();
        let mut recent: HeaplessVec<&str, MAX_STREAM_LINES> = HeaplessVec::new();
        for worker in self.workers.iter().rev() {
            #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
            if !worker.target_published {
                continue;
            }
            let worker_label = worker_shard_label(worker.id.as_str(), sharding);
            if worker_label == label {
                if recent.push(worker.id.as_str()).is_err() {
                    break;
                }
            }
        }
        for worker_id in recent.iter().rev() {
            push_list_entry(output, worker_id)?;
        }
        Ok(())
    }

    fn handle_queen_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        if matches!(self.session_role, Some(SessionRoleLabel::WorkerBus)) {
            return Err(NineDoorBridgeError::Permission);
        }
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        self.sync_target_worker_projections()?;
        let command = parse_queen_ctl(payload)?;
        match command {
            QueenCtlCommand::Spawn(target) => {
                self.ensure_lifecycle_gate(lifecycle::GATE_NEW_WORK)?;
                match target {
                    SpawnTarget::Gpu => self.spawn_gpu_from_ctl(payload),
                    _ => self.spawn_worker(target),
                }
            }
            QueenCtlCommand::Kill(worker_id) => self.remove_worker(worker_id),
            QueenCtlCommand::Bind { from, to } => {
                self.ensure_lifecycle_gate(lifecycle::GATE_NEW_WORK)?;
                self.bind_namespace(from, to)
            }
            QueenCtlCommand::Mount { service, at } => {
                self.ensure_lifecycle_gate(lifecycle::GATE_NEW_WORK)?;
                self.mount_namespace(service, at)
            }
        }
    }

    fn spawn_gpu_from_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        if !self.gpu.enabled() {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        if self.workers.len() >= MAX_WORKERS {
            return Err(NineDoorBridgeError::BufferFull);
        }
        let gpu_id = parse_json_string_field(payload, "gpu_id")
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        let mem_mb = u32::try_from(
            parse_json_u64_field(payload, "mem_mb").ok_or(NineDoorBridgeError::InvalidPayload)?,
        )
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        let streams = u8::try_from(
            parse_json_u64_field(payload, "streams").ok_or(NineDoorBridgeError::InvalidPayload)?,
        )
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        let ttl_s = u32::try_from(
            parse_json_u64_field(payload, "ttl_s").ok_or(NineDoorBridgeError::InvalidPayload)?,
        )
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        let priority = u8::try_from(parse_json_u64_field(payload, "priority").unwrap_or(0))
            .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        if mem_mb == 0 || streams == 0 || ttl_s == 0 {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if self.gpu.entry(gpu_id).is_none() {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let (worker_id, worker_label) = self.allocate_worker_identity()?;
        let line = format!(
            "{{\"schema\":\"{}\",\"state\":\"{}\",\"gpu_id\":\"{}\",\"worker_id\":\"{}\",\"mem_mb\":{mem_mb},\"streams\":{streams},\"ttl_s\":{ttl_s},\"priority\":{priority}}}",
            GPU_LEASE_SCHEMA,
            GPU_LEASE_ACTIVE_STATE,
            escape_json_string(gpu_id),
            escape_json_string(worker_label.as_str())
        );
        let lease_max = self.gpu.lease_max_bytes;
        let before_len = {
            let entry = self
                .gpu
                .entry_mut(gpu_id)
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let before_len = entry.lease_log.len();
            append_log_bytes(&mut entry.lease_log, &line, lease_max)?;
            before_len
        };
        if let Err(err) = self.spawn_worker_with_identity(SpawnTarget::Gpu, worker_id, worker_label)
        {
            if let Some(entry) = self.gpu.entry_mut(gpu_id) {
                entry.lease_log.truncate(before_len);
            }
            return Err(err);
        }
        Ok(())
    }

    fn handle_lifecycle_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        let command =
            lifecycle::parse_command(payload).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        let now_ms = crate::hal::timebase().now_ms();
        let outstanding = self.workers.len();
        match lifecycle::apply_command(command, now_ms, outstanding) {
            Ok(transition) => {
                let line = lifecycle::format_transition_log(&transition);
                log_buffer::append_log_line(line.as_str());
                Ok(())
            }
            Err(err) => {
                let line = lifecycle::format_denied_log(lifecycle::state(), payload.trim(), err);
                log_buffer::append_log_line(line.as_str());
                Err(lifecycle_error_to_bridge(err))
            }
        }
    }

    fn spawn_worker(&mut self, target: SpawnTarget) -> Result<(), NineDoorBridgeError> {
        let (worker_id, label) = self.allocate_worker_identity()?;
        self.spawn_worker_with_identity(target, worker_id, label)
    }

    fn spawn_worker_with_identity(
        &mut self,
        target: SpawnTarget,
        worker_id: u32,
        id: HeaplessString<MAX_WORKER_ID_LEN>,
    ) -> Result<(), NineDoorBridgeError> {
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        {
            if self.workers.len() >= MAX_WORKERS {
                return Err(NineDoorBridgeError::BufferFull);
            }
            self.workers
                .try_reserve(1)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
            let snapshot = enqueue_target_worker_spawn(target.worker_role(), id.as_str())
                .map_err(worker_target_error)?;
            let identity = snapshot
                .identity
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let target_index =
                flat_slot_index(snapshot.role, identity.slot).map_err(worker_target_error)?;
            let worker_index = self.workers.len();
            let mut worker = WorkerTelemetry {
                id,
                ring: TelemetryRing::new(self.telemetry.ring_bytes_per_worker as usize),
                target,
                target_lifecycle: snapshot.lifecycle,
                target_identity: snapshot.identity,
                target_revision: 0,
                target_ready_sequence: 0,
                target_receipt_sequence: 0,
                target_completion_sequence: 0,
                target_published: false,
            };
            worker.apply_target_snapshot(snapshot)?;
            self.workers.push(worker);
            self.target_worker_indexes[target_index] = Some(worker_index);
            let _ = worker_id;
            return Ok(());
        }

        #[cfg(not(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)))]
        {
            let policy = affinity::policy();
            let worker_index = worker_id.saturating_sub(1) as usize;
            affinity::with_role_affinity(
                affinity::AffinityRole::Worker,
                worker_index,
                &policy,
                || {
                    if self.workers.len() >= MAX_WORKERS {
                        return Err(NineDoorBridgeError::BufferFull);
                    }
                    let ring = TelemetryRing::new(self.telemetry.ring_bytes_per_worker as usize);
                    self.workers
                        .try_reserve(1)
                        .map_err(|_| NineDoorBridgeError::BufferFull)?;
                    self.workers.push(WorkerTelemetry { id, ring, target });
                    Ok(())
                },
            )
        }
    }

    fn allocate_worker_identity(
        &self,
    ) -> Result<(u32, HeaplessString<MAX_WORKER_ID_LEN>), NineDoorBridgeError> {
        let max_workers =
            u32::try_from(MAX_WORKERS).map_err(|_| NineDoorBridgeError::BufferFull)?;
        for worker_id in 1..=max_workers {
            let mut id = HeaplessString::<MAX_WORKER_ID_LEN>::new();
            write!(id, "worker-{worker_id}").map_err(|_| NineDoorBridgeError::BufferFull)?;
            if !self
                .workers
                .iter()
                .any(|worker| worker.id.as_str() == id.as_str())
            {
                return Ok((worker_id, id));
            }
        }
        Err(NineDoorBridgeError::BufferFull)
    }

    fn remove_worker(&mut self, worker_id: &str) -> Result<(), NineDoorBridgeError> {
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        {
            enqueue_target_worker_kill(worker_id).map_err(worker_target_error)?;
            self.sync_target_worker_projections()?;
            return Ok(());
        }

        #[cfg(not(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)))]
        {
            let position = self
                .workers
                .iter()
                .position(|worker| worker.id.as_str() == worker_id)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            let _ = self.workers.swap_remove(position);
            Ok(())
        }
    }

    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    fn sync_target_worker_projections(&mut self) -> Result<(), NineDoorBridgeError> {
        for snapshot in target_worker_namespace_snapshots() {
            if snapshot.public_id().is_none() {
                continue;
            }
            self.apply_target_worker_projection(snapshot)?;
        }
        Ok(())
    }

    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    fn sync_target_worker_projection_by_public_id(
        &mut self,
        public_id: &str,
    ) -> Result<TargetWorkerNamespaceSnapshot, NineDoorBridgeError> {
        let snapshot = target_worker_namespace_snapshot_by_public_id(public_id)
            .map_err(|_| NineDoorBridgeError::InvalidPath)?;
        self.apply_target_worker_projection(snapshot)?;
        Ok(snapshot)
    }

    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    fn sync_target_worker_projection_for_identity(
        &mut self,
        identity: WorkerIdentity,
    ) -> Result<TargetWorkerNamespaceSnapshot, NineDoorBridgeError> {
        let snapshot =
            target_worker_namespace_snapshot_for_identity(identity).map_err(worker_target_error)?;
        self.apply_target_worker_projection(snapshot)?;
        Ok(snapshot)
    }

    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    fn apply_target_worker_projection(
        &mut self,
        snapshot: TargetWorkerNamespaceSnapshot,
    ) -> Result<(), NineDoorBridgeError> {
        let identity = snapshot
            .identity
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        let target_index =
            flat_slot_index(snapshot.role, identity.slot).map_err(worker_target_error)?;
        let worker_index =
            self.target_worker_indexes[target_index].ok_or(NineDoorBridgeError::InvalidPath)?;
        let worker = self
            .workers
            .get_mut(worker_index)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        worker.apply_target_snapshot(snapshot)
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

    fn mount_namespace(&mut self, service: &str, at: &str) -> Result<(), NineDoorBridgeError> {
        validate_bind_path(at)?;
        let canonical = generated::namespace_mounts()
            .iter()
            .find(|mount| mount.service == service)
            .map(|mount| join_path("", mount.target))
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let target = normalize_path(at);
        if canonical == target {
            return Ok(());
        }
        self.bind_namespace(canonical.as_str(), target.as_str())
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
        self.ensure_lifecycle_gate(lifecycle::GATE_WORKER_TELEMETRY)?;
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        self.sync_target_worker_projection_by_public_id(worker_id)?;
        let worker = self
            .workers
            .iter_mut()
            .find(|worker| worker.id.as_str() == worker_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        if worker.target_lifecycle != crate::worker_supervisor::WorkerLifecycleState::Ready
            || !worker.target_published
        {
            return Err(NineDoorBridgeError::Permission);
        }
        if matches!(
            self.telemetry.frame_schema,
            generated::TelemetryFrameSchema::CborV1
        ) {
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
        if matches!(self.session_role, Some(SessionRoleLabel::WorkerBus)) {
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
            _ => None,
        }
    }

    fn log_sidecar_denial(&self, kind: SidecarKind) {
        let scope = self.session_scope().unwrap_or("none");
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(line, "sidecar-deny kind={} scope={}", kind.as_str(), scope);
        log_buffer::append_log_line(line.as_str());
    }

    fn log_lifecycle_gate_denial(&self, action: &str) {
        let state = lifecycle::state();
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "lifecycle denied action={} state={} reason=gate-denied",
            action,
            lifecycle::state_label(state)
        );
        log_buffer::append_log_line(line.as_str());
    }

    fn ensure_lifecycle_gate(
        &self,
        gate: lifecycle::LifecycleGate,
    ) -> Result<(), NineDoorBridgeError> {
        if lifecycle::gate_allows(gate) {
            Ok(())
        } else {
            self.log_lifecycle_gate_denial(gate.name);
            Err(NineDoorBridgeError::Permission)
        }
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

    fn apply_policy_gate(&mut self, path: &str) -> Result<PolicyGateDecision, NineDoorBridgeError> {
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
        let mut output = HeaplessVec::new();
        self.action_status_lines_into(action_id, &mut output)?;
        Ok(output)
    }

    fn action_status_lines_into(
        &self,
        action_id: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let action = self
            .policy
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let decision = action.decision.as_str();
        let state = if action.consumed {
            "consumed"
        } else {
            "queued"
        };
        let max_len = core::cmp::min(
            self.policy.limits.status_max_bytes as usize,
            DEFAULT_LINE_CAPACITY,
        );
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let wrote = write!(
            line,
            "{{\"id\":\"{}\",\"decision\":\"{}\",\"state\":\"{}\"}}",
            action.id, decision, state
        )
        .is_ok();
        if !wrote || line.len() > max_len {
            line.clear();
            let _ = write!(line, "{{\"id\":\"{}\",\"state\":\"oversize\"}}", action.id);
        }
        output.clear();
        push_boot_line(output, line.as_str())
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
    } else if role.eq_ignore_ascii_case("worker") || role.eq_ignore_ascii_case("worker-heartbeat") {
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
    proc_9p_session: generated::Proc9pSessionConfig,
    proc_root: generated::ProcRootConfig,
    proc_pressure: generated::ProcPressureConfig,
    snapshot: IngestSnapshot,
    watch: IngestWatch,
}

impl ObserveState {
    fn new() -> Self {
        let config = generated::observability_config();
        Self {
            proc_ingest: config.proc_ingest,
            proc_9p_session: config.proc_9p_session,
            proc_root: config.proc_root,
            proc_pressure: config.proc_pressure,
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

    fn proc_root_enabled(&self) -> bool {
        self.proc_root.reachable || self.proc_root.last_seen_ms || self.proc_root.cut_reason
    }

    fn proc_pressure_enabled(&self) -> bool {
        self.proc_pressure.busy
            || self.proc_pressure.quota
            || self.proc_pressure.cut
            || self.proc_pressure.policy
    }

    fn proc_9p_session_enabled(&self) -> bool {
        self.proc_9p_session.active
            || self.proc_9p_session.state
            || self.proc_9p_session.since_ms
            || self.proc_9p_session.owner
    }

    fn update_ingest_snapshot(&mut self, snapshot: IngestSnapshot) {
        self.snapshot = snapshot;
    }

    fn ingest_lines(
        &self,
        path: &str,
    ) -> Option<
        Result<
            HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
            NineDoorBridgeError,
        >,
    > {
        let mut output = HeaplessVec::new();
        match self.ingest_lines_into(path, &mut output) {
            Some(Ok(())) => Some(Ok(output)),
            Some(Err(err)) => Some(Err(err)),
            None => None,
        }
    }

    fn ingest_lines_into(
        &self,
        path: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Option<Result<(), NineDoorBridgeError>> {
        output.clear();
        match path {
            PROC_INGEST_P50_PATH if self.proc_ingest.p50_ms => Some(
                render_p50_line(self.snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_INGEST_P95_PATH if self.proc_ingest.p95_ms => Some(
                render_p95_line(self.snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_INGEST_BACKPRESSURE_PATH if self.proc_ingest.backpressure => Some(
                render_backpressure_line(self.snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_INGEST_DROPPED_PATH if self.proc_ingest.dropped => Some(
                render_dropped_line(self.snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_INGEST_QUEUED_PATH if self.proc_ingest.queued => Some(
                render_queued_line(self.snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_INGEST_WATCH_PATH if self.proc_ingest.watch => Some(self.watch.lines_into(output)),
            _ => None,
        }
    }

    fn root_lines(
        &self,
        path: &str,
    ) -> Option<
        Result<
            HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
            NineDoorBridgeError,
        >,
    > {
        let mut output = HeaplessVec::new();
        match self.root_lines_into(path, &mut output) {
            Some(Ok(())) => Some(Ok(output)),
            Some(Err(err)) => Some(Err(err)),
            None => None,
        }
    }

    fn root_lines_into(
        &self,
        path: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Option<Result<(), NineDoorBridgeError>> {
        output.clear();
        let snapshot = lifecycle::root_snapshot();
        match path {
            PROC_ROOT_REACHABLE_PATH if self.proc_root.reachable => Some(
                render_root_reachable_line(snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_ROOT_LAST_SEEN_PATH if self.proc_root.last_seen_ms => Some(
                render_root_last_seen_line(snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_ROOT_CUT_REASON_PATH if self.proc_root.cut_reason => Some(
                render_root_cut_reason_line(snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            _ => None,
        }
    }

    fn pressure_lines(
        &self,
        path: &str,
    ) -> Option<
        Result<
            HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
            NineDoorBridgeError,
        >,
    > {
        let mut output = HeaplessVec::new();
        match self.pressure_lines_into(path, &mut output) {
            Some(Ok(())) => Some(Ok(output)),
            Some(Err(err)) => Some(Err(err)),
            None => None,
        }
    }

    fn pressure_lines_into(
        &self,
        path: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Option<Result<(), NineDoorBridgeError>> {
        output.clear();
        let snapshot = crate::observe::pressure_snapshot();
        match path {
            PROC_PRESSURE_BUSY_PATH if self.proc_pressure.busy => Some(
                render_pressure_busy_line(snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_PRESSURE_QUOTA_PATH if self.proc_pressure.quota => Some(
                render_pressure_quota_line(snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_PRESSURE_CUT_PATH if self.proc_pressure.cut => Some(
                render_pressure_cut_line(snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            PROC_PRESSURE_POLICY_PATH if self.proc_pressure.policy => Some(
                render_pressure_policy_line(snapshot)
                    .and_then(|line| lines_from_text_into(line.as_str(), output)),
            ),
            _ => None,
        }
    }

    fn watch_lines(
        &mut self,
        now_ms: u64,
        audit: &mut dyn AuditSink,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        self.watch_lines_into(now_ms, audit, &mut output)?;
        Ok(output)
    }

    fn watch_lines_into(
        &mut self,
        now_ms: u64,
        audit: &mut dyn AuditSink,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.proc_ingest.watch {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        self.watch.maybe_append(now_ms, self.snapshot, audit)?;
        output.clear();
        self.watch.lines_into(output)
    }

    fn list_ingest_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.proc_ingest_enabled() {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        if self.proc_ingest.p50_ms {
            push_list_entry(output, "p50_ms")?;
        }
        if self.proc_ingest.p95_ms {
            push_list_entry(output, "p95_ms")?;
        }
        if self.proc_ingest.backpressure {
            push_list_entry(output, "backpressure")?;
        }
        if self.proc_ingest.dropped {
            push_list_entry(output, "dropped")?;
        }
        if self.proc_ingest.queued {
            push_list_entry(output, "queued")?;
        }
        if self.proc_ingest.watch {
            push_list_entry(output, "watch")?;
        }
        Ok(())
    }

    fn list_root_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.proc_root_enabled() {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        if self.proc_root.reachable {
            push_list_entry(output, "reachable")?;
        }
        if self.proc_root.last_seen_ms {
            push_list_entry(output, "last_seen_ms")?;
        }
        if self.proc_root.cut_reason {
            push_list_entry(output, "cut_reason")?;
        }
        Ok(())
    }

    fn list_pressure_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        if !self.proc_pressure_enabled() {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        if self.proc_pressure.busy {
            push_list_entry(output, "busy")?;
        }
        if self.proc_pressure.quota {
            push_list_entry(output, "quota")?;
        }
        if self.proc_pressure.cut {
            push_list_entry(output, "cut")?;
        }
        if self.proc_pressure.policy {
            push_list_entry(output, "policy")?;
        }
        Ok(())
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
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        self.lines_into(&mut output)?;
        Ok(output)
    }

    fn lines_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        output.clear();
        for entry in self.entries.iter() {
            let mut line = HeaplessString::new();
            line.push_str(entry.as_str())
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
            output
                .push(line)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        Ok(())
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
    UpdateEpoch {
        epoch: String,
    },
    UpdateManifest {
        epoch: String,
    },
    UpdateStatus {
        epoch: String,
        cbor: bool,
    },
    UpdateChunks {
        epoch: String,
    },
    UpdateChunk {
        epoch: String,
        digest: [u8; 32],
    },
    ModelsRoot,
    ModelRoot {
        digest: [u8; 32],
    },
    ModelFile {
        digest: [u8; 32],
        kind: ModelFileKind,
    },
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

    fn list_path_into(
        &mut self,
        path: &str,
        is_queen: bool,
        ui_updates_manifest: bool,
        ui_updates_status: bool,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<bool, NineDoorBridgeError> {
        let Some(cas_path) = parse_cas_path(path)? else {
            return Ok(false);
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
        output.clear();
        for entry in entries {
            push_list_entry(output, entry.as_str())?;
        }
        Ok(true)
    }

    fn read_manifest(&self, epoch: &str) -> Result<Vec<u8>, NineDoorBridgeError> {
        let bundle = self
            .updates
            .get(epoch)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let data = bundle
            .manifest_bytes
            .as_deref()
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        Ok(data.to_vec())
    }

    fn read_chunk(&self, digest: &[u8; 32]) -> Result<Vec<u8>, NineDoorBridgeError> {
        let data = self
            .chunks
            .get(digest)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        Ok(data.clone())
    }

    fn read_model_file(
        &self,
        digest: &[u8; 32],
        kind: ModelFileKind,
    ) -> Result<Vec<u8>, NineDoorBridgeError> {
        let model = self
            .models
            .get(digest)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
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
        let payload = decode_cas_payload(data)?;
        let (decoded, manifest_bytes) = {
            let bundle = self
                .updates
                .get_mut(epoch)
                .expect("update bundle must exist");
            if let Some(existing) = bundle.manifest_bytes.as_ref() {
                let expected_offset = bundle.manifest_pending.len() as u64;
                let provided_offset = if offset == u64::MAX {
                    expected_offset
                } else {
                    offset
                };
                if provided_offset != expected_offset {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let new_len = bundle.manifest_pending.len().saturating_add(payload.len());
                if new_len > existing.len() {
                    bundle.manifest_pending.clear();
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                bundle.manifest_pending.extend_from_slice(&payload);
                if !existing.starts_with(bundle.manifest_pending.as_slice()) {
                    bundle.manifest_pending.clear();
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if bundle.manifest_pending.len() == existing.len() {
                    bundle.manifest_pending.clear();
                }
                return Ok(data.len() as u32);
            }
            let expected_offset = bundle.manifest_pending.len() as u64;
            let provided_offset = if offset == u64::MAX {
                expected_offset
            } else {
                offset
            };
            if provided_offset != expected_offset {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let new_len = bundle.manifest_pending.len().saturating_add(payload.len());
            if new_len > CAS_MANIFEST_MAX_BYTES {
                return Err(NineDoorBridgeError::BufferFull);
            }
            bundle.manifest_pending.extend_from_slice(&payload);
            match CasManifest::decode(&bundle.manifest_pending) {
                Ok(manifest) => (Some(manifest), Some(bundle.manifest_pending.clone())),
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
            let pending = self.pending_chunks.entry(*digest).or_default();
            let expected_offset = pending.len() as u64;
            let provided_offset = if offset == u64::MAX {
                expected_offset
            } else {
                offset
            };
            if provided_offset != expected_offset {
                pending.clear();
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let new_len = pending.len().saturating_add(payload.len());
            if new_len > existing.len() {
                pending.clear();
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            pending.extend_from_slice(&payload);
            if !existing.starts_with(pending.as_slice()) {
                pending.clear();
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if pending.len() == existing.len() {
                pending.clear();
            }
            return Ok(data.len() as u32);
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
        let expected_bytes =
            (manifest.chunks.len() as u64).saturating_mul(manifest.chunk_bytes as u64);
        if manifest.payload_bytes != expected_bytes {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if manifest.chunks.len() > CAS_MANIFEST_MAX_CHUNKS {
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
            let key = self.config.verification_key.ok_or_else(|| {
                self.log_event(&format!(
                    "cas-manifest rejected epoch={} reason=verification-key-missing",
                    epoch
                ));
                NineDoorBridgeError::Permission
            })?;
            let verifying_key = VerifyingKey::from_bytes(&key).map_err(|_| {
                self.log_event(&format!(
                    "cas-manifest rejected epoch={} reason=verification-key-invalid",
                    epoch
                ));
                NineDoorBridgeError::Permission
            })?;
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
        let delta_label = if manifest.delta.is_some() {
            "delta"
        } else {
            "base"
        };
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
                let chunk = self
                    .chunks
                    .get(digest)
                    .ok_or(NineDoorBridgeError::InvalidPath)?;
                payload.extend_from_slice(chunk);
            }
        }
        for digest in &manifest.chunks {
            let chunk = self
                .chunks
                .get(digest)
                .ok_or(NineDoorBridgeError::InvalidPath)?;
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
        let max_bytes = self.chunk_bytes().saturating_mul(CAS_MANIFEST_MAX_CHUNKS);
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostTicketV2RawSpec {
    schema: String,
    id: String,
    idempotency_key: String,
    action: String,
    args: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_unix_ms: Option<u64>,
    receipt_mode: String,
    operation_id: String,
    subject_ref: String,
    receipt_worker_role: String,
    receipt_worker_id: String,
    receipt_supervisor_generation: u64,
    receipt_cap_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostTicketV2AdmittedSpec {
    schema: String,
    id: String,
    idempotency_key: String,
    action: String,
    args: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_unix_ms: Option<u64>,
    receipt_mode: String,
    operation_id: String,
    subject_ref: String,
    receipt_worker_role: String,
    receipt_worker_id: String,
    receipt_supervisor_generation: u64,
    receipt_cap_generation: u64,
    resolved_worker_slot: u16,
    resolved_lease_epoch: u64,
    admission_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostTicketV2Result {
    schema: String,
    id: String,
    idempotency_key: String,
    action: String,
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    receipt_mode: String,
    operation_id: String,
    subject_ref: String,
    receipt_worker_role: String,
    receipt_worker_id: String,
    receipt_supervisor_generation: u64,
    receipt_cap_generation: u64,
    resolved_worker_slot: u16,
    resolved_lease_epoch: u64,
    admission_sequence: u64,
    result_digest: String,
}

#[derive(Debug, Clone)]
struct HostTicketV2Admission {
    spec: HostTicketV2AdmittedSpec,
    raw_digest: [u8; 32],
    terminal_result_digest: Option<[u8; 32]>,
    terminal_outcome: Option<WorkerOutcome>,
}

#[derive(Debug, Clone, Copy)]
struct HostTicketV2WorkerBinding<'a> {
    public_id: &'a str,
    role: WorkerRole,
    identity: WorkerIdentity,
    ready: bool,
    ready_sequence: u64,
    current_control_sequence: u64,
    last_control_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostTicketV2TerminalDisposition {
    Submit(WorkerOutcome),
    Stale,
}

#[derive(Debug)]
struct HostEntry {
    path: String,
    value: String,
    control: Option<&'static str>,
    writable: bool,
    ticket_log: bool,
    retained_wire_lines: usize,
    base_offset: u64,
    next_offset: u64,
    dropped_lines: u64,
    dropped_bytes: u64,
}

#[derive(Debug)]
struct HostState {
    enabled: bool,
    mount_at: String,
    mount_parts: Vec<String>,
    providers: &'static [generated::HostProvider],
    tickets_enabled: bool,
    ticket_request_schema: &'static str,
    ticket_result_schema: &'static str,
    ticket_accepted_request_schemas: &'static [&'static str],
    ticket_accepted_result_schemas: &'static [&'static str],
    ticket_max_line_bytes: u32,
    ticket_action_allowlist: &'static [generated::HostTicketAction],
    ticket_receipt_action_allowlist: &'static [generated::HostTicketAction],
    ticket_lifecycle: &'static [generated::HostTicketLifecycleState],
    next_admission_sequence: u64,
    admissions: BTreeMap<[u8; 32], HostTicketV2Admission>,
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
            tickets_enabled: config.tickets.enable,
            ticket_request_schema: config.tickets.request_schema,
            ticket_result_schema: config.tickets.result_schema,
            ticket_accepted_request_schemas: config.tickets.accepted_request_schemas,
            ticket_accepted_result_schemas: config.tickets.accepted_result_schemas,
            ticket_max_line_bytes: config.tickets.max_line_bytes,
            ticket_action_allowlist: config.tickets.action_allowlist,
            ticket_receipt_action_allowlist: config.tickets.receipt_action_allowlist,
            ticket_lifecycle: config.tickets.lifecycle,
            next_admission_sequence: 1,
            admissions: BTreeMap::new(),
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

    fn list_into(
        &self,
        path: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Option<Result<(), NineDoorBridgeError>> {
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
                output.clear();
                return Some(list_from_slice_into(&[next.as_str()], output));
            }
            return None;
        }
        if parts.len() == self.mount_parts.len() {
            if !self.mount_parts_match(&parts) {
                return None;
            }
            output.clear();
            if self.tickets_enabled && push_list_entry(output, "tickets").is_err() {
                return Some(Err(NineDoorBridgeError::BufferFull));
            }
            for provider in self.providers.iter().copied() {
                let label = host_provider_label(provider);
                if push_list_entry(output, label).is_err() {
                    return Some(Err(NineDoorBridgeError::BufferFull));
                }
            }
            return Some(Ok(()));
        }
        if !self.mount_parts_match(&parts) {
            return None;
        }
        let rel = &parts[self.mount_parts.len()..];
        match rel {
            ["tickets"] if self.tickets_enabled => {
                output.clear();
                Some(list_from_slice_into(
                    &[
                        "spec",
                        "status",
                        "deadletter",
                        "spec.snapshot",
                        "status.snapshot",
                        "deadletter.snapshot",
                        "retention",
                        "current",
                    ],
                    output,
                ))
            }
            ["tickets", "current"] if self.tickets_enabled => {
                output.clear();
                Some(Ok(()))
            }
            [provider]
                if self
                    .providers
                    .iter()
                    .copied()
                    .any(|candidate| host_provider_label(candidate) == *provider) =>
            {
                output.clear();
                Some(list_from_slice_into(&["status"], output))
            }
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

    fn entry_tail_window(&self, path: &str) -> Option<(&str, u64)> {
        if !self.enabled {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| {
                let base_offset = if entry.ticket_log {
                    entry.base_offset
                } else {
                    0
                };
                (entry.value.as_str(), base_offset)
            })
    }

    fn is_ticket_retention_path(&self, path: &str) -> bool {
        self.tickets_enabled && path == format!("{}/tickets/retention", self.mount_at)
    }

    fn retention_lines_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        output.clear();
        for entry in self.entries.iter().filter(|entry| entry.ticket_log) {
            let label = entry
                .path
                .rsplit('/')
                .next()
                .ok_or(NineDoorBridgeError::InvalidPath)?;
            let mut line = HeaplessString::new();
            write!(
                line,
                "HOST_TICKET_RETENTION schema=v1 path={} base={} next={} retained_bytes={} retained_wire_lines={} dropped_lines={} dropped_bytes={}",
                label,
                entry.base_offset,
                entry.next_offset,
                entry.value.len(),
                entry.retained_wire_lines,
                entry.dropped_lines,
                entry.dropped_bytes,
            )
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
            output
                .push(line)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        Ok(())
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

    fn writable(&self, path: &str) -> bool {
        if !self.enabled {
            return false;
        }
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.writable)
            .unwrap_or(false)
    }

    fn validate_append(&self, path: &str, payload: &str) -> Result<(), NineDoorBridgeError> {
        if !self.tickets_enabled {
            return Ok(());
        }
        if path.ends_with("/tickets/spec") {
            return self.validate_v1_ticket_spec_lines(payload);
        }
        if path.ends_with("/tickets/status") || path.ends_with("/tickets/deadletter") {
            return self.validate_v1_ticket_result_lines(payload);
        }
        Ok(())
    }

    fn is_ticket_write_path(&self, path: &str) -> bool {
        self.tickets_enabled
            && (path.ends_with("/tickets/spec")
                || path.ends_with("/tickets/status")
                || path.ends_with("/tickets/deadletter"))
    }

    fn update_value(&mut self, path: &str, value: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) {
            let retained_wire_lines = if entry.ticket_log {
                let Ok(lines) = cat_wire_line_count(value) else {
                    return false;
                };
                if value.len() > HOST_TICKET_LOG_MAX_BYTES || lines > MAX_STREAM_LINES {
                    return false;
                }
                lines
            } else {
                0
            };
            entry.value = String::from(value);
            entry.retained_wire_lines = retained_wire_lines;
            entry.base_offset = 0;
            entry.next_offset = value.len() as u64;
            entry.dropped_lines = 0;
            entry.dropped_bytes = 0;
            return true;
        }
        false
    }

    fn append_ticket_lines(
        &mut self,
        path: &str,
        lines: &[String],
    ) -> Result<(), NineDoorBridgeError> {
        self.can_append_ticket_lines(path, lines)?;
        self.append_ticket_lines_preflighted(path, lines)
    }

    fn append_ticket_lines_preflighted(
        &mut self,
        path: &str,
        lines: &[String],
    ) -> Result<(), NineDoorBridgeError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.path == path && entry.ticket_log)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        for line in lines {
            let appended_bytes = line
                .len()
                .checked_add(1)
                .ok_or(NineDoorBridgeError::BufferFull)?;
            let appended_wire_lines = cat_wire_line_count(line.as_str())?;
            while entry.value.len().saturating_add(appended_bytes) > HOST_TICKET_LOG_MAX_BYTES
                || entry
                    .retained_wire_lines
                    .saturating_add(appended_wire_lines)
                    > MAX_STREAM_LINES
            {
                if entry.value.is_empty() {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                let content_end = entry.value.find('\n').unwrap_or(entry.value.len());
                let removed_bytes = if content_end < entry.value.len() {
                    content_end.saturating_add(1)
                } else {
                    content_end
                };
                let removed_wire_lines = cat_wire_line_count(&entry.value[..content_end])?;
                entry.value.drain(..removed_bytes);
                entry.retained_wire_lines =
                    entry.retained_wire_lines.saturating_sub(removed_wire_lines);
                entry.base_offset = entry
                    .base_offset
                    .checked_add(removed_bytes as u64)
                    .ok_or(NineDoorBridgeError::BufferFull)?;
                entry.dropped_lines = entry.dropped_lines.saturating_add(1);
                entry.dropped_bytes = entry.dropped_bytes.saturating_add(removed_bytes as u64);
            }
            entry.value.push_str(line);
            entry.value.push('\n');
            entry.retained_wire_lines = entry
                .retained_wire_lines
                .checked_add(appended_wire_lines)
                .ok_or(NineDoorBridgeError::BufferFull)?;
            entry.next_offset = entry
                .next_offset
                .checked_add(appended_bytes as u64)
                .ok_or(NineDoorBridgeError::BufferFull)?;
        }
        Ok(())
    }

    fn ticket_snapshot_path(&self, path: &str) -> Result<String, NineDoorBridgeError> {
        if path.ends_with("/tickets/spec")
            || path.ends_with("/tickets/status")
            || path.ends_with("/tickets/deadletter")
        {
            return Ok(format!("{path}.snapshot"));
        }
        Err(NineDoorBridgeError::InvalidPath)
    }

    fn can_append_ticket_lines(
        &self,
        path: &str,
        lines: &[String],
    ) -> Result<(), NineDoorBridgeError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        if !entry.ticket_log {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let mut projected_bytes = 0usize;
        let mut projected_wire_lines = 0usize;
        for line in lines {
            self.validate_ticket_line_bytes(line)?;
            projected_bytes = projected_bytes
                .checked_add(line.len().saturating_add(1))
                .ok_or(NineDoorBridgeError::BufferFull)?;
            projected_wire_lines = projected_wire_lines
                .checked_add(cat_wire_line_count(line.as_str())?)
                .ok_or(NineDoorBridgeError::BufferFull)?;
        }
        if projected_bytes > HOST_TICKET_LOG_MAX_BYTES || projected_wire_lines > MAX_STREAM_LINES {
            return Err(NineDoorBridgeError::BufferFull);
        }
        entry
            .next_offset
            .checked_add(projected_bytes as u64)
            .ok_or(NineDoorBridgeError::BufferFull)?;
        Ok(())
    }

    fn accepted_request_schema(&self, schema: &str) -> bool {
        self.ticket_accepted_request_schemas.contains(&schema)
    }

    fn accepted_result_schema(&self, schema: &str) -> bool {
        self.ticket_accepted_result_schemas.contains(&schema)
    }

    fn receipt_action(&self, action: &str) -> bool {
        self.ticket_receipt_action_allowlist
            .iter()
            .any(|allowed| host_ticket_action_label(*allowed) == action)
    }

    fn validate_v1_ticket_spec_lines(&self, payload: &str) -> Result<(), NineDoorBridgeError> {
        let mut saw_line = false;
        for raw_line in payload.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            saw_line = true;
            self.validate_ticket_line_bytes(line)?;
            validate_json_keys(
                line,
                &[
                    "schema",
                    "id",
                    "idempotency_key",
                    "action",
                    "target",
                    "args",
                    "expires_unix_ms",
                    "source_hive",
                    "target_hive",
                    "relay_hop",
                    "relay_correlation_id",
                    "receipt_mode",
                ],
            )?;
            let schema = parse_json_string_field(line, "schema")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            if schema != self.ticket_request_schema
                || !self.accepted_request_schema(schema)
                || schema == HOST_TICKET_V2_REQUEST_SCHEMA
            {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let id =
                parse_json_string_field(line, "id").ok_or(NineDoorBridgeError::InvalidPayload)?;
            let idempotency_key = parse_json_string_field(line, "idempotency_key")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let action = parse_json_string_field(line, "action")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_host_ticket_token(id)?;
            validate_host_ticket_token(idempotency_key)?;
            if !self
                .ticket_action_allowlist
                .iter()
                .any(|allowed| host_ticket_action_label(*allowed) == action)
            {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if self.receipt_action(action) {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if parse_json_string_field(line, "receipt_mode").is_some_and(|mode| mode != "none") {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if let Some(target) = parse_json_string_field(line, "target") {
                if target.trim().is_empty() {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
            if let Some(expires_unix_ms) = parse_json_u64_field(line, "expires_unix_ms") {
                if expires_unix_ms == 0 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
            let source_hive = parse_json_string_field(line, "source_hive");
            let target_hive = parse_json_string_field(line, "target_hive");
            if source_hive.is_some() != target_hive.is_some() {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if let Some(source_hive) = source_hive {
                validate_host_ticket_token(source_hive)?;
            }
            if let Some(target_hive) = target_hive {
                validate_host_ticket_token(target_hive)?;
            }
            if let Some(relay_hop) = parse_json_u64_field(line, "relay_hop") {
                if relay_hop == 0 || relay_hop > 32 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
            if let Some(correlation) = parse_json_string_field(line, "relay_correlation_id") {
                validate_host_ticket_token(correlation)?;
            }
        }
        if !saw_line {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        Ok(())
    }

    fn validate_v1_ticket_result_lines(&self, payload: &str) -> Result<(), NineDoorBridgeError> {
        let mut saw_line = false;
        for raw_line in payload.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            saw_line = true;
            self.validate_ticket_line_bytes(line)?;
            validate_json_keys(
                line,
                &[
                    "schema",
                    "id",
                    "idempotency_key",
                    "action",
                    "state",
                    "message",
                    "source_hive",
                    "target_hive",
                    "relay_hop",
                    "relay_correlation_id",
                    "receipt_mode",
                ],
            )?;
            let schema = parse_json_string_field(line, "schema")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            if schema != self.ticket_result_schema
                || !self.accepted_result_schema(schema)
                || schema == HOST_TICKET_V2_RESULT_SCHEMA
            {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let id =
                parse_json_string_field(line, "id").ok_or(NineDoorBridgeError::InvalidPayload)?;
            let idempotency_key = parse_json_string_field(line, "idempotency_key")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let action = parse_json_string_field(line, "action")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let state = parse_json_string_field(line, "state")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_host_ticket_token(id)?;
            validate_host_ticket_token(idempotency_key)?;
            if !self
                .ticket_action_allowlist
                .iter()
                .any(|allowed| host_ticket_action_label(*allowed) == action)
            {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if self.receipt_action(action) {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if parse_json_string_field(line, "receipt_mode").is_some_and(|mode| mode != "none") {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if !self
                .ticket_lifecycle
                .iter()
                .any(|allowed| host_ticket_lifecycle_label(*allowed) == state)
            {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if let Some(message) = parse_json_string_field(line, "message") {
                if message.trim().is_empty() {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
            let source_hive = parse_json_string_field(line, "source_hive");
            let target_hive = parse_json_string_field(line, "target_hive");
            if source_hive.is_some() != target_hive.is_some() {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            if let Some(source_hive) = source_hive {
                validate_host_ticket_token(source_hive)?;
            }
            if let Some(target_hive) = target_hive {
                validate_host_ticket_token(target_hive)?;
            }
            if let Some(relay_hop) = parse_json_u64_field(line, "relay_hop") {
                if relay_hop == 0 || relay_hop > 32 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
            if let Some(correlation) = parse_json_string_field(line, "relay_correlation_id") {
                validate_host_ticket_token(correlation)?;
            }
        }
        if !saw_line {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        Ok(())
    }

    fn validate_ticket_line_bytes(&self, line: &str) -> Result<(), NineDoorBridgeError> {
        if line.len() > self.ticket_max_line_bytes as usize {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        Ok(())
    }

    fn has_provider(&self, provider: generated::HostProvider) -> bool {
        self.providers.iter().any(|entry| *entry == provider)
    }

    fn mount_parts_prefix(&self, parts: &HeaplessVec<&str, MAX_POLICY_PATH_COMPONENTS>) -> bool {
        for (idx, part) in parts.iter().enumerate() {
            if self.mount_parts.get(idx).map(String::as_str) != Some(*part) {
                return false;
            }
        }
        true
    }

    fn mount_parts_match(&self, parts: &HeaplessVec<&str, MAX_POLICY_PATH_COMPONENTS>) -> bool {
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
        if self.tickets_enabled {
            self.push_entry(&["tickets", "spec"], "", Some("tickets.spec"));
            self.push_entry(&["tickets", "status"], "", Some("tickets.status"));
            self.push_entry(&["tickets", "deadletter"], "", Some("tickets.deadletter"));
            self.push_read_only_entry(&["tickets", "spec.snapshot"], "");
            self.push_read_only_entry(&["tickets", "status.snapshot"], "");
            self.push_read_only_entry(&["tickets", "deadletter.snapshot"], "");
        }
        for provider in self.providers.iter().copied() {
            self.push_read_only_entry(
                &[host_provider_label(provider), "status"],
                "unavailable source=none",
            );
        }
    }

    fn push_entry(&mut self, parts: &[&str], value: &str, control: Option<&'static str>) {
        let path = join_path(self.mount_at.as_str(), parts);
        self.entries.push(HostEntry {
            path,
            value: String::from(value),
            control,
            writable: true,
            ticket_log: parts.first() == Some(&"tickets"),
            retained_wire_lines: usize::from(!value.is_empty()),
            base_offset: 0,
            next_offset: value.len() as u64,
            dropped_lines: 0,
            dropped_bytes: 0,
        });
    }

    fn push_read_only_entry(&mut self, parts: &[&str], value: &str) {
        let path = join_path(self.mount_at.as_str(), parts);
        self.entries.push(HostEntry {
            path,
            value: String::from(value),
            control: None,
            writable: false,
            ticket_log: parts.first() == Some(&"tickets"),
            retained_wire_lines: usize::from(!value.is_empty()),
            base_offset: 0,
            next_offset: value.len() as u64,
            dropped_lines: 0,
            dropped_bytes: 0,
        });
    }
}

#[derive(Debug)]
struct GpuEntry {
    id: String,
    info_payload: String,
    ctl_log: Vec<u8>,
    lease_log: Vec<u8>,
    status_log: Vec<u8>,
}

#[derive(Debug)]
struct GpuModelManifest {
    model_id: String,
    manifest_toml: String,
    manifest_sha256: String,
    cas_sha256: String,
    base_model_id: Option<String>,
    adapter_sha256: Option<String>,
}

#[derive(Debug)]
struct GpuSnapshotIdentity {
    source_id: String,
    source_mode: String,
    epoch: u64,
    sequence: u64,
    observed_unix_ms: u64,
    ttl_ms: u64,
    catalog_sha256: String,
    available: bool,
}

#[derive(Debug)]
struct GpuBridgePending {
    expected_bytes: usize,
    expected_sha256: [u8; 32],
    encoded: Vec<u8>,
}

#[derive(Debug)]
struct GpuBridgeState {
    ctl_log: Vec<u8>,
    status: Vec<u8>,
    pending: Option<GpuBridgePending>,
}

#[derive(Debug)]
struct GpuBridgeSnapshot {
    identity: GpuSnapshotIdentity,
    entries: Vec<GpuEntry>,
    models: Vec<GpuModelManifest>,
    active: String,
    activation_generation: u64,
    activation_receipt: String,
    telemetry_schema: Vec<u8>,
}

#[derive(Debug)]
enum GpuBridgeUpdate {
    None,
    Started {
        bytes: usize,
    },
    Complete {
        bytes: usize,
        sha256: String,
        snapshot: GpuBridgeSnapshot,
    },
}

#[derive(Debug)]
struct GpuState {
    enabled: bool,
    ctl_max_bytes: u32,
    lease_max_bytes: u32,
    status_max_bytes: u32,
    entries: Vec<GpuEntry>,
    models: Vec<GpuModelManifest>,
    models_active_log: Vec<u8>,
    telemetry_schema: Vec<u8>,
    accepted_identity: Option<GpuSnapshotIdentity>,
    expires_at_ms: Option<u64>,
    bridge: GpuBridgeState,
}

impl GpuState {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ctl_max_bytes: GPU_CTL_MAX_BYTES,
            lease_max_bytes: GPU_LEASE_MAX_BYTES,
            status_max_bytes: GPU_STATUS_MAX_BYTES,
            entries: Vec::new(),
            models: Vec::new(),
            models_active_log: Vec::new(),
            telemetry_schema: Vec::new(),
            accepted_identity: None,
            expires_at_ms: None,
            bridge: GpuBridgeState {
                ctl_log: Vec::new(),
                status: if enabled {
                    b"state=unavailable source=none\n".to_vec()
                } else {
                    Vec::new()
                },
                pending: None,
            },
        }
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn entry(&self, id: &str) -> Option<&GpuEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    fn entry_mut(&mut self, id: &str) -> Option<&mut GpuEntry> {
        self.entries.iter_mut().find(|entry| entry.id == id)
    }

    fn models_ready(&self) -> bool {
        !self.models.is_empty() || !self.models_active_log.is_empty()
    }

    fn telemetry_ready(&self) -> bool {
        !self.telemetry_schema.is_empty()
    }

    fn qemu_lora_export_ready(&self) -> bool {
        qemu_lora_export_fixture_allowed(
            self.accepted_identity
                .as_ref()
                .map_or("none", |identity| identity.source_mode.as_str()),
            cfg!(all(feature = "bootstrap-trace", feature = "release-qemu")),
            self.expires_at_ms.is_some(),
        )
    }

    fn handle_bridge_payload(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        for line in payload.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("begin") {
                self.bridge.ctl_log.clear();
                self.bridge.pending = None;
            }
            append_log_bytes(&mut self.bridge.ctl_log, trimmed, GPU_BRIDGE_CTL_MAX_BYTES)?;
            match self.handle_bridge_line(trimmed) {
                Ok(GpuBridgeUpdate::None) => {}
                Ok(GpuBridgeUpdate::Started { bytes }) => {
                    self.set_bridge_status(&format!("state=receiving bytes={bytes}"))?;
                }
                Ok(GpuBridgeUpdate::Complete {
                    bytes,
                    sha256,
                    snapshot,
                }) => {
                    let state = if snapshot.identity.available {
                        "ok"
                    } else {
                        "unavailable"
                    };
                    let source = snapshot.identity.source_id.clone();
                    let source_mode = snapshot.identity.source_mode.clone();
                    let epoch = snapshot.identity.epoch;
                    let sequence = snapshot.identity.sequence;
                    let ttl_ms = snapshot.identity.ttl_ms;
                    self.apply_bridge_snapshot(snapshot)?;
                    self.set_bridge_status(&format!(
                        "state={state} source={source} mode={source_mode} epoch={epoch} sequence={sequence} ttl_ms={ttl_ms} bytes={bytes} sha256={sha256}"
                    ))?;
                    if source_mode == "fixture" {
                        log::info!(
                            "GPU_BRIDGE_FIXTURE_ADMISSION source={} mode=fixture profile=qemu gate=bootstrap-trace state=admitted",
                            source,
                        );
                        log::info!(
                            "LORA_EXPORT_FIXTURE_ADMISSION source={} job={} mode=fixture profile=qemu gate=bootstrap-trace state=admitted",
                            source,
                            QEMU_LORA_EXPORT_JOB_ID,
                        );
                    }
                }
                Err(err) => {
                    let _ = self.set_bridge_status("state=err");
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    fn handle_bridge_line(&mut self, line: &str) -> Result<GpuBridgeUpdate, NineDoorBridgeError> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(GpuBridgeUpdate::None);
        }
        if let Some(rest) = trimmed.strip_prefix("begin") {
            let (expected_bytes, expected_sha256) = parse_gpu_bridge_begin(rest)?;
            if expected_bytes > GPU_BRIDGE_MAX_BYTES {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            self.bridge.pending = Some(GpuBridgePending {
                expected_bytes,
                expected_sha256,
                encoded: Vec::new(),
            });
            return Ok(GpuBridgeUpdate::Started {
                bytes: expected_bytes,
            });
        }
        if trimmed == "end" {
            let pending = self
                .bridge
                .pending
                .take()
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let decoded = BASE64_STANDARD
                .decode(&pending.encoded)
                .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            if decoded.len() != pending.expected_bytes {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let mut hasher = Sha256::new();
            hasher.update(&decoded);
            let digest = hasher.finalize();
            if digest.as_slice() != pending.expected_sha256 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let sha256 = hex::encode(digest);
            let snapshot = parse_gpu_bridge_wire(&decoded)?;
            return Ok(GpuBridgeUpdate::Complete {
                bytes: pending.expected_bytes,
                sha256,
                snapshot,
            });
        }
        if let Some(rest) = trimmed.strip_prefix("b64:") {
            let pending = self
                .bridge
                .pending
                .as_mut()
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let expected_len = base64_encoded_len(pending.expected_bytes)
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            if pending.encoded.len().saturating_add(rest.len()) > expected_len {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            pending.encoded.extend_from_slice(rest.as_bytes());
            return Ok(GpuBridgeUpdate::None);
        }
        Err(NineDoorBridgeError::InvalidPayload)
    }

    fn apply_bridge_snapshot(
        &mut self,
        snapshot: GpuBridgeSnapshot,
    ) -> Result<(), NineDoorBridgeError> {
        if let Some(current) = self.accepted_identity.as_ref() {
            if current.source_id == snapshot.identity.source_id
                && (snapshot.identity.epoch < current.epoch
                    || (snapshot.identity.epoch == current.epoch
                        && snapshot.identity.sequence <= current.sequence))
            {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
        }
        let active_line = if snapshot.active.is_empty() {
            String::new()
        } else {
            format!("{}\n", snapshot.active)
        };
        if active_line.len() > GPU_MODELS_ACTIVE_MAX_BYTES as usize {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if snapshot.telemetry_schema.len() > GPU_TELEMETRY_SCHEMA_MAX_BYTES {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        self.entries = snapshot.entries;
        self.models = snapshot.models;
        self.models_active_log = active_line.into_bytes();
        self.telemetry_schema = snapshot.telemetry_schema;
        let now_ms = crate::hal::timebase().now_ms();
        self.expires_at_ms = Some(now_ms.saturating_add(snapshot.identity.ttl_ms));
        self.accepted_identity = Some(snapshot.identity);
        Ok(())
    }

    fn withdraw_expired(&mut self, now_ms: u64) {
        let Some(expires_at_ms) = self.expires_at_ms else {
            return;
        };
        if now_ms < expires_at_ms {
            return;
        }
        let source = self
            .accepted_identity
            .as_ref()
            .map(|identity| identity.source_id.clone())
            .unwrap_or_else(|| "none".to_owned());
        self.entries.clear();
        self.models.clear();
        self.models_active_log.clear();
        self.telemetry_schema.clear();
        self.expires_at_ms = None;
        let _ =
            self.set_bridge_status(&format!("state=unavailable source={source} reason=expired"));
    }

    fn set_bridge_status(&mut self, line: &str) -> Result<(), NineDoorBridgeError> {
        let mut payload = String::from(line);
        if !payload.ends_with('\n') {
            payload.push('\n');
        }
        if payload.len() > GPU_BRIDGE_STATUS_MAX_BYTES {
            payload.truncate(GPU_BRIDGE_STATUS_MAX_BYTES);
            if !payload.ends_with('\n') {
                payload.push('\n');
            }
        }
        self.bridge.status = payload.into_bytes();
        Ok(())
    }
}

fn parse_gpu_bridge_begin(payload: &str) -> Result<(usize, [u8; 32]), NineDoorBridgeError> {
    let mut bytes = None;
    let mut sha256 = None;
    for part in payload.split_whitespace() {
        let (key, value) = part
            .split_once('=')
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        match key {
            "bytes" => {
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
                if parsed == 0 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                bytes = Some(parsed);
            }
            "sha256" => sha256 = Some(value),
            _ => return Err(NineDoorBridgeError::InvalidPayload),
        }
    }
    let bytes = bytes.ok_or(NineDoorBridgeError::InvalidPayload)?;
    let sha256 = sha256.ok_or(NineDoorBridgeError::InvalidPayload)?;
    let sha256 = parse_sha256(sha256)?;
    Ok((bytes, sha256))
}

fn base64_encoded_len(bytes: usize) -> Option<usize> {
    let blocks = bytes / 3;
    let rem = bytes % 3;
    let base = blocks.checked_mul(4)?;
    let extra = if rem == 0 { 0 } else { 4 };
    base.checked_add(extra)
}

fn parse_gpu_bridge_wire(bytes: &[u8]) -> Result<GpuBridgeSnapshot, NineDoorBridgeError> {
    let text = core::str::from_utf8(bytes).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let mut schema_seen = false;
    let mut identity: Option<GpuSnapshotIdentity> = None;
    let mut entries = Vec::new();
    let mut models = Vec::new();
    let mut active: Option<String> = None;
    let mut active_contract: Option<(u64, String, String)> = None;
    let mut telemetry_schema: Option<Vec<u8>> = None;
    let mut ended = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if ended {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if line == "end" {
            ended = true;
            continue;
        }
        let mut parts = line.split_whitespace();
        let keyword = parts.next().ok_or(NineDoorBridgeError::InvalidPayload)?;
        match keyword {
            "schema" => {
                if schema_seen {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let schema = parts.next().ok_or(NineDoorBridgeError::InvalidPayload)?;
                if schema != GPU_BRIDGE_WIRE_SCHEMA {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if parts.next().is_some() {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                schema_seen = true;
            }
            "snapshot" => {
                if identity.is_some() {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let mut source_id = None;
                let mut source_mode = None;
                let mut epoch = None;
                let mut sequence = None;
                let mut observed_unix_ms = None;
                let mut ttl_ms = None;
                let mut catalog_sha256 = None;
                let mut available = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(NineDoorBridgeError::InvalidPayload)?;
                    match key {
                        "source" if source_id.is_none() => source_id = Some(value),
                        "mode" if source_mode.is_none() => source_mode = Some(value),
                        "epoch" if epoch.is_none() => epoch = Some(parse_gpu_positive_u64(value)?),
                        "sequence" if sequence.is_none() => {
                            sequence = Some(parse_gpu_positive_u64(value)?)
                        }
                        "observed_unix_ms" if observed_unix_ms.is_none() => {
                            observed_unix_ms = Some(parse_gpu_positive_u64(value)?)
                        }
                        "ttl_ms" if ttl_ms.is_none() => {
                            ttl_ms = Some(parse_gpu_positive_u64(value)?)
                        }
                        "catalog_sha256" if catalog_sha256.is_none() => {
                            catalog_sha256 = Some(value)
                        }
                        "available" if available.is_none() => {
                            available = Some(match value {
                                "0" => false,
                                "1" => true,
                                _ => return Err(NineDoorBridgeError::InvalidPayload),
                            })
                        }
                        _ => return Err(NineDoorBridgeError::InvalidPayload),
                    }
                }
                let source_id = source_id.ok_or(NineDoorBridgeError::InvalidPayload)?;
                validate_gpu_source_id(source_id)?;
                let source_mode = source_mode.ok_or(NineDoorBridgeError::InvalidPayload)?;
                if !gpu_snapshot_mode_allowed(
                    source_mode,
                    cfg!(all(feature = "bootstrap-trace", feature = "release-qemu")),
                ) {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let ttl_ms = ttl_ms.ok_or(NineDoorBridgeError::InvalidPayload)?;
                if ttl_ms > GPU_BRIDGE_MAX_TTL_MS {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let catalog_sha256 = catalog_sha256.ok_or(NineDoorBridgeError::InvalidPayload)?;
                validate_gpu_sha256(catalog_sha256)?;
                identity = Some(GpuSnapshotIdentity {
                    source_id: source_id.to_owned(),
                    source_mode: source_mode.to_owned(),
                    epoch: epoch.ok_or(NineDoorBridgeError::InvalidPayload)?,
                    sequence: sequence.ok_or(NineDoorBridgeError::InvalidPayload)?,
                    observed_unix_ms: observed_unix_ms
                        .ok_or(NineDoorBridgeError::InvalidPayload)?,
                    ttl_ms,
                    catalog_sha256: catalog_sha256.to_owned(),
                    available: available.ok_or(NineDoorBridgeError::InvalidPayload)?,
                });
            }
            "node" => {
                let mut id = None;
                let mut info = None;
                let mut ctl = None;
                let mut lease = None;
                let mut status = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(NineDoorBridgeError::InvalidPayload)?;
                    match key {
                        "id" if id.is_none() => id = Some(value),
                        "info" if info.is_none() => info = Some(value),
                        "ctl" if ctl.is_none() => ctl = Some(value),
                        "lease" if lease.is_none() => lease = Some(value),
                        "status" if status.is_none() => status = Some(value),
                        _ => return Err(NineDoorBridgeError::InvalidPayload),
                    }
                }
                let id = id.ok_or(NineDoorBridgeError::InvalidPayload)?;
                validate_gpu_id(id)?;
                if entries.iter().any(|entry: &GpuEntry| entry.id == id) {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let info_payload = decode_gpu_bridge_string(info)?;
                let ctl_log = decode_gpu_bridge_bytes(ctl)?;
                let lease_log = decode_gpu_bridge_bytes(lease)?;
                let status_log = decode_gpu_bridge_bytes(status)?;
                entries.push(GpuEntry {
                    id: id.to_owned(),
                    info_payload,
                    ctl_log,
                    lease_log,
                    status_log,
                });
            }
            "model" => {
                let mut id = None;
                let mut manifest = None;
                let mut manifest_sha256 = None;
                let mut cas_sha256 = None;
                let mut base_model_id = None;
                let mut adapter_sha256 = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(NineDoorBridgeError::InvalidPayload)?;
                    match key {
                        "id" if id.is_none() => id = Some(value),
                        "manifest" if manifest.is_none() => manifest = Some(value),
                        "manifest_sha256" if manifest_sha256.is_none() => {
                            manifest_sha256 = Some(value)
                        }
                        "cas_sha256" if cas_sha256.is_none() => cas_sha256 = Some(value),
                        "base" if base_model_id.is_none() => base_model_id = Some(value),
                        "adapter_sha256" if adapter_sha256.is_none() => {
                            adapter_sha256 = Some(value)
                        }
                        _ => return Err(NineDoorBridgeError::InvalidPayload),
                    }
                }
                let id = id.ok_or(NineDoorBridgeError::InvalidPayload)?;
                validate_model_id(id)?;
                if models
                    .iter()
                    .any(|model: &GpuModelManifest| model.model_id == id)
                {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let manifest_toml = decode_gpu_bridge_string(manifest)?;
                if manifest_toml.len() > GPU_MODEL_MANIFEST_MAX_BYTES {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let manifest_sha256 = manifest_sha256.ok_or(NineDoorBridgeError::InvalidPayload)?;
                validate_gpu_sha256(manifest_sha256)?;
                if gpu_sha256_hex(manifest_toml.as_bytes()) != manifest_sha256 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let cas_sha256 = cas_sha256.ok_or(NineDoorBridgeError::InvalidPayload)?;
                validate_gpu_sha256(cas_sha256)?;
                let base_model_id = parse_gpu_optional_identity(
                    base_model_id.ok_or(NineDoorBridgeError::InvalidPayload)?,
                    validate_model_id,
                )?;
                let adapter_sha256 = parse_gpu_optional_identity(
                    adapter_sha256.ok_or(NineDoorBridgeError::InvalidPayload)?,
                    validate_gpu_sha256,
                )?;
                if adapter_sha256.is_some() && base_model_id.is_none() {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                models.push(GpuModelManifest {
                    model_id: id.to_owned(),
                    manifest_toml,
                    manifest_sha256: manifest_sha256.to_owned(),
                    cas_sha256: cas_sha256.to_owned(),
                    base_model_id,
                    adapter_sha256,
                });
            }
            "active" => {
                let mut id = None;
                let mut generation = None;
                let mut receipt = None;
                let mut manifest_sha256 = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(NineDoorBridgeError::InvalidPayload)?;
                    match key {
                        "id" if id.is_none() => id = Some(value),
                        "generation" if generation.is_none() => {
                            generation = Some(
                                value
                                    .parse::<u64>()
                                    .map_err(|_| NineDoorBridgeError::InvalidPayload)?,
                            )
                        }
                        "receipt" if receipt.is_none() => receipt = Some(value),
                        "manifest_sha256" if manifest_sha256.is_none() => {
                            manifest_sha256 = Some(value)
                        }
                        _ => return Err(NineDoorBridgeError::InvalidPayload),
                    }
                }
                let id = id.ok_or(NineDoorBridgeError::InvalidPayload)?;
                let generation = generation.ok_or(NineDoorBridgeError::InvalidPayload)?;
                let receipt = receipt.ok_or(NineDoorBridgeError::InvalidPayload)?;
                let manifest_sha256 = manifest_sha256.ok_or(NineDoorBridgeError::InvalidPayload)?;
                if id == GPU_BRIDGE_EMPTY_VALUE {
                    if generation != 0
                        || receipt != GPU_BRIDGE_EMPTY_VALUE
                        || manifest_sha256 != GPU_BRIDGE_EMPTY_VALUE
                    {
                        return Err(NineDoorBridgeError::InvalidPayload);
                    }
                    active = Some(String::new());
                } else {
                    validate_model_id(id)?;
                    if generation == 0 {
                        return Err(NineDoorBridgeError::InvalidPayload);
                    }
                    validate_gpu_sha256(receipt)?;
                    validate_gpu_sha256(manifest_sha256)?;
                    active = Some(id.to_owned());
                }
                active_contract =
                    Some((generation, receipt.to_owned(), manifest_sha256.to_owned()));
            }
            "telemetry" => {
                if telemetry_schema.is_some() {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let mut schema = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or(NineDoorBridgeError::InvalidPayload)?;
                    match key {
                        "schema" if schema.is_none() => schema = Some(value),
                        _ => return Err(NineDoorBridgeError::InvalidPayload),
                    }
                }
                let schema = decode_gpu_bridge_bytes(schema)?;
                if schema.len() > GPU_TELEMETRY_SCHEMA_MAX_BYTES {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                telemetry_schema = Some(schema);
            }
            _ => return Err(NineDoorBridgeError::InvalidPayload),
        }
    }

    if !schema_seen || !ended {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let identity = identity.ok_or(NineDoorBridgeError::InvalidPayload)?;
    if identity.available != !models.is_empty() {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if gpu_catalog_sha256(&models) != identity.catalog_sha256 {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    for model in &models {
        if let Some(base) = model.base_model_id.as_deref() {
            if !models.iter().any(|candidate| candidate.model_id == base) {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
        }
    }
    let active = active.ok_or(NineDoorBridgeError::InvalidPayload)?;
    let (activation_generation, activation_receipt, active_manifest_sha256) =
        active_contract.ok_or(NineDoorBridgeError::InvalidPayload)?;
    if !active.is_empty()
        && !models
            .iter()
            .any(|model| model.model_id.as_str() == active.as_str())
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if !active.is_empty() {
        let model = models
            .iter()
            .find(|model| model.model_id == active)
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        if model.manifest_sha256 != active_manifest_sha256
            || gpu_activation_receipt(
                &identity.source_id,
                identity.epoch,
                activation_generation,
                &active,
                &model.manifest_sha256,
                &identity.catalog_sha256,
            ) != activation_receipt
        {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
    }
    let telemetry_schema = telemetry_schema.ok_or(NineDoorBridgeError::InvalidPayload)?;
    Ok(GpuBridgeSnapshot {
        identity,
        entries,
        models,
        active,
        activation_generation,
        activation_receipt,
        telemetry_schema,
    })
}

fn gpu_snapshot_mode_allowed(source_mode: &str, qemu_evidence_gate: bool) -> bool {
    source_mode == "production" || (qemu_evidence_gate && source_mode == "fixture")
}

fn qemu_lora_export_fixture_allowed(
    source_mode: &str,
    qemu_evidence_gate: bool,
    snapshot_live: bool,
) -> bool {
    source_mode == "fixture" && qemu_evidence_gate && snapshot_live
}

fn decode_gpu_bridge_string(value: Option<&str>) -> Result<String, NineDoorBridgeError> {
    let bytes = decode_gpu_bridge_bytes(value)?;
    String::from_utf8(bytes).map_err(|_| NineDoorBridgeError::InvalidPayload)
}

fn parse_gpu_positive_u64(value: &str) -> Result<u64, NineDoorBridgeError> {
    let value = value
        .parse::<u64>()
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    if value == 0 {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(value)
}

fn parse_gpu_optional_identity(
    value: &str,
    validate: fn(&str) -> Result<(), NineDoorBridgeError>,
) -> Result<Option<String>, NineDoorBridgeError> {
    if value == GPU_BRIDGE_EMPTY_VALUE {
        return Ok(None);
    }
    validate(value)?;
    Ok(Some(value.to_owned()))
}

fn validate_gpu_source_id(value: &str) -> Result<(), NineDoorBridgeError> {
    if value.is_empty()
        || value.len() > GPU_MODEL_ID_MAX_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_gpu_sha256(value: &str) -> Result<(), NineDoorBridgeError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn gpu_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn gpu_catalog_sha256(models: &[GpuModelManifest]) -> String {
    let mut hasher = Sha256::new();
    for model in models {
        hasher.update(model.model_id.as_bytes());
        hasher.update([0]);
        hasher.update(model.manifest_sha256.as_bytes());
        hasher.update([0]);
        hasher.update(model.cas_sha256.as_bytes());
        hasher.update([0]);
        hasher.update(
            model
                .base_model_id
                .as_deref()
                .unwrap_or(GPU_BRIDGE_EMPTY_VALUE)
                .as_bytes(),
        );
        hasher.update([0]);
        hasher.update(
            model
                .adapter_sha256
                .as_deref()
                .unwrap_or(GPU_BRIDGE_EMPTY_VALUE)
                .as_bytes(),
        );
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

fn gpu_activation_receipt(
    source_id: &str,
    epoch: u64,
    generation: u64,
    model_id: &str,
    manifest_sha256: &str,
    catalog_sha256: &str,
) -> String {
    let epoch = epoch.to_string();
    let generation = generation.to_string();
    let mut hasher = Sha256::new();
    for field in [
        source_id,
        epoch.as_str(),
        generation.as_str(),
        model_id,
        manifest_sha256,
        catalog_sha256,
    ] {
        hasher.update(field.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

fn decode_gpu_bridge_bytes(value: Option<&str>) -> Result<Vec<u8>, NineDoorBridgeError> {
    let value = value.ok_or(NineDoorBridgeError::InvalidPayload)?;
    BASE64_STANDARD
        .decode(value.as_bytes())
        .map_err(|_| NineDoorBridgeError::InvalidPayload)
}

fn validate_gpu_id(value: &str) -> Result<(), NineDoorBridgeError> {
    if value.is_empty() || value.len() > MAX_WORKER_ID_LEN {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if value == "." || value == ".." || value.contains('/') {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_model_id(value: &str) -> Result<(), NineDoorBridgeError> {
    if value.is_empty() || value.len() > GPU_MODEL_ID_MAX_BYTES {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if value == "." || value == ".." || value.contains('/') {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarKind {
    Bus,
}

impl SidecarKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bus => "bus",
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
}

impl SidecarState {
    fn new() -> Self {
        let config = generated::sidecar_config();
        let bus = SidecarBusState::new(config.modbus, config.dnp3);
        Self { bus }
    }

    fn push_root_entries(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let mut seen: Vec<String> = Vec::new();
        self.bus.push_root_entries(output, &mut seen)?;
        Ok(())
    }

    fn kind_for_path(&self, path: &[&str]) -> Option<SidecarKind> {
        if self.bus.matches_path(path) {
            return Some(SidecarKind::Bus);
        }
        None
    }

    fn allowed_prefix(&self, kind: SidecarKind, scope: Option<&str>, path: &[&str]) -> bool {
        match kind {
            SidecarKind::Bus => self.bus.allowed_prefix(scope, path),
        }
    }

    fn allowed_path(&self, kind: SidecarKind, scope: Option<&str>, path: &[&str]) -> bool {
        match kind {
            SidecarKind::Bus => self.bus.allowed_path(scope, path),
        }
    }

    fn list_into(
        &self,
        path: &[&str],
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Option<Result<(), NineDoorBridgeError>> {
        self.bus.list_into(path, output)
    }

    fn read(&self, path: &[&str]) -> Option<Vec<u8>> {
        self.bus.read(path)
    }

    fn write(&mut self, path: &[&str], payload: &[u8]) -> Result<Option<u32>, NineDoorBridgeError> {
        if let Some(count) = self.bus.write(path, payload, SIDECAR_LOG_MAX_BYTES)? {
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

    fn push_adapters(
        adapters: &mut Vec<SidecarBusAdapterState>,
        config: generated::SidecarBusConfig,
    ) {
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

    fn list_into(
        &self,
        path: &[&str],
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Option<Result<(), NineDoorBridgeError>> {
        if self.adapters.is_empty() {
            return None;
        }
        let mut matched_root = false;
        for adapter in &self.adapters {
            let root_len = adapter.mount_root.len().saturating_sub(1);
            let root = &adapter.mount_root[..root_len];
            if segments_equal(path, root) {
                if !matched_root {
                    output.clear();
                }
                matched_root = true;
                if push_list_entry(output, adapter.mount_label.as_str()).is_err() {
                    return Some(Err(NineDoorBridgeError::BufferFull));
                }
            }
        }
        if matched_root {
            return Some(Ok(()));
        }
        for adapter in &self.adapters {
            if segments_equal(path, &adapter.mount_root) {
                output.clear();
                return Some(list_from_slice_into(
                    &["ctl", "telemetry", "link", "replay", "spool"],
                    output,
                ));
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
            SidecarBusFile::Spool => {
                Some(render_spool_status(&adapter.spool, SIDECAR_LOG_MAX_BYTES))
            }
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

#[derive(Debug, Clone)]
struct PolicyRevision {
    id: String,
    sha256: String,
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
    current: Option<PolicyRevision>,
    previous: Option<PolicyRevision>,
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
            current: None,
            previous: None,
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
        let _ = writeln!(
            text,
            "req total={} queued={} consumed={}",
            total, queued, consumed
        );
        for action in &self.actions {
            let state = if action.consumed {
                "consumed"
            } else {
                "queued"
            };
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
        writer
            .text("total")
            .and_then(|_| writer.unsigned(total as u64))
            .map_err(cbor_error)?;
        writer
            .text("queued")
            .and_then(|_| writer.unsigned(queued as u64))
            .map_err(cbor_error)?;
        writer
            .text("consumed")
            .and_then(|_| writer.unsigned(consumed as u64))
            .map_err(cbor_error)?;
        writer
            .text("actions")
            .and_then(|_| writer.array(self.actions.len()))
            .map_err(cbor_error)?;
        for action in &self.actions {
            let state = if action.consumed {
                "consumed"
            } else {
                "queued"
            };
            writer
                .map(4)
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
        writer
            .text("rules")
            .and_then(|_| writer.unsigned(self.rules.len() as u64))
            .map_err(cbor_error)?;
        writer
            .text("actions")
            .and_then(|_| writer.unsigned(self.actions.len() as u64))
            .map_err(cbor_error)?;
        writer
            .text("unmatched")
            .and_then(|_| writer.unsigned(unmatched as u64))
            .map_err(cbor_error)?;
        writer
            .text("entries")
            .and_then(|_| writer.array(self.rules.len()))
            .map_err(cbor_error)?;
        for (rule, (queued, consumed)) in self.rules.iter().zip(rule_counts.iter()) {
            writer
                .map(4)
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
        let command = parse_policy_ctl(payload)?;
        let (next_current, next_previous) = match command {
            PolicyCtlCommand::Apply { id, sha256 } => {
                let next = PolicyRevision { id, sha256 };
                (Some(next), self.current.clone())
            }
            PolicyCtlCommand::Rollback { id } => {
                let current = self
                    .current
                    .as_ref()
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                if current.id != id {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                (self.previous.clone(), None)
            }
        };
        append_log_bytes(&mut self.ctl_log, payload, self.limits.ctl_max_bytes)?;
        self.current = next_current;
        self.previous = next_previous;
        Ok(())
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
        self.evict_replaced_consumed_actions(&actions);
        self.evict_consumed_actions(actions.len(), max_entries);
        if self.actions.len() + actions.len() > max_entries {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        for (index, action) in actions.iter().enumerate() {
            if self
                .actions
                .iter()
                .any(|entry| !entry.consumed && entry.id == action.id)
                || actions[..index].iter().any(|prior| prior.id == action.id)
            {
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

    fn evict_replaced_consumed_actions(&mut self, incoming: &[PolicyAction]) {
        self.actions.retain(|entry| {
            !(entry.consumed && incoming.iter().any(|action| action.id == entry.id))
        });
    }

    fn evict_consumed_actions(&mut self, incoming: usize, max_entries: usize) {
        while self.actions.len().saturating_add(incoming) > max_entries {
            let Some(index) = self.actions.iter().position(|action| action.consumed) else {
                break;
            };
            let _ = self.actions.remove(index);
        }
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
                PolicyDecision::Approve => {
                    PolicyGateDecision::Allowed(PolicyGateAllowance::Action {
                        id: action.id.clone(),
                        target: action.target.clone(),
                    })
                }
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
struct ScheduleEntry {
    id: String,
    role: String,
    priority: u32,
    ticks: u32,
    budget_ms: u32,
    seq: u64,
}

#[derive(Debug)]
struct ScheduleState {
    enabled: bool,
    queue_max_entries: usize,
    ctl_max_bytes: u32,
    ctl_log: Vec<u8>,
    queue: VecDeque<ScheduleEntry>,
    dequeued: u64,
    dropped: u64,
    next_seq: u64,
    proc_summary: bool,
    proc_queue: bool,
    proc_summary_bytes: usize,
    proc_queue_bytes: usize,
}

impl ScheduleState {
    fn new(
        control: generated::ScheduleControlConfig,
        observability: generated::ProcScheduleConfig,
    ) -> Self {
        Self {
            enabled: control.enable,
            queue_max_entries: control.queue_max_entries as usize,
            ctl_max_bytes: control.ctl_max_bytes,
            ctl_log: Vec::new(),
            queue: VecDeque::new(),
            dequeued: 0,
            dropped: 0,
            next_seq: 1,
            proc_summary: observability.summary,
            proc_queue: observability.queue,
            proc_summary_bytes: observability.summary_bytes as usize,
            proc_queue_bytes: observability.queue_bytes as usize,
        }
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn proc_enabled(&self) -> bool {
        self.proc_summary || self.proc_queue
    }

    fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    fn list_proc_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        if self.proc_summary {
            push_list_entry(output, "summary")?;
        }
        if self.proc_queue {
            push_list_entry(output, "queue")?;
        }
        Ok(())
    }

    fn append_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        if !self.enabled {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        match parse_schedule_ctl(payload)? {
            ScheduleCtlCommand::Enqueue(request) => {
                if self.queue_max_entries == 0 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if self.queue.len() >= self.queue_max_entries {
                    self.dropped = self.dropped.saturating_add(1);
                    return Err(NineDoorBridgeError::BufferFull);
                }
                if self.queue.iter().any(|entry| entry.id == request.id) {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                self.queue.push_back(ScheduleEntry {
                    id: request.id,
                    role: request.role,
                    priority: request.priority,
                    ticks: request.ticks,
                    budget_ms: request.budget_ms,
                    seq,
                });
            }
            ScheduleCtlCommand::Dequeue { id } => {
                let front = self
                    .queue
                    .front()
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                if front.id != id {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let _ = self.queue.pop_front();
                self.dequeued = self.dequeued.saturating_add(1);
            }
        }
        Ok(())
    }

    fn summary_lines(
        &self,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        self.summary_lines_into(&mut output)?;
        Ok(output)
    }

    fn summary_lines_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "queue={} dequeued={} dropped={} max_entries={}",
            self.queue.len(),
            self.dequeued,
            self.dropped,
            self.queue_max_entries
        );
        if line.len() > self.proc_summary_bytes || line.len() > OBSERVE_SCHEDULE_SUMMARY_BYTES {
            return Err(NineDoorBridgeError::BufferFull);
        }
        output.clear();
        lines_from_text_into(line.as_str(), output)
    }

    fn queue_lines(
        &self,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        self.queue_lines_into(&mut output)?;
        Ok(output)
    }

    fn queue_lines_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let mut out = String::new();
        for entry in &self.queue {
            let line = format!(
                "id={} role={} priority={} ticks={} budget_ms={} seq={}\n",
                entry.id, entry.role, entry.priority, entry.ticks, entry.budget_ms, entry.seq
            );
            if line.len() > DEFAULT_LINE_CAPACITY {
                return Err(NineDoorBridgeError::BufferFull);
            }
            if !push_bounded_line(&mut out, &line, self.proc_queue_bytes) {
                break;
            }
        }
        output.clear();
        lines_from_bytes_into(out.as_bytes(), output)
    }
}

#[derive(Debug, Clone)]
struct LeaseEntry {
    id: String,
    subject: String,
    resource: String,
    ttl_s: u32,
    priority: u32,
    state: &'static str,
    seq: u64,
    last_request_tag: Option<[u8; LEASE_REQUEST_TAG_BYTES]>,
}

#[derive(Debug, Clone)]
struct LeasePreemption {
    id: String,
    subject: String,
    resource: String,
    reason: String,
    seq: u64,
}

#[derive(Debug, Clone)]
struct LeaseQuota {
    subject: String,
    resource: String,
    max_active: u32,
    max_preemptions: u32,
}

#[derive(Debug)]
struct LeaseState {
    enabled: bool,
    active_max_entries: usize,
    preemptions_max_entries: usize,
    ctl_max_bytes: u32,
    ctl_log: Vec<u8>,
    active: Vec<LeaseEntry>,
    preemptions: VecDeque<LeasePreemption>,
    preemptions_total: u64,
    quotas: Vec<LeaseQuota>,
    next_seq: u64,
    proc_summary: bool,
    proc_active: bool,
    proc_preemptions: bool,
    proc_summary_bytes: usize,
    proc_active_bytes: usize,
    proc_preemptions_bytes: usize,
}

impl LeaseState {
    fn new(
        control: generated::LeaseControlConfig,
        observability: generated::ProcLeaseConfig,
    ) -> Self {
        Self {
            enabled: control.enable,
            active_max_entries: control.active_max_entries as usize,
            preemptions_max_entries: control.preemptions_max_entries as usize,
            ctl_max_bytes: control.ctl_max_bytes,
            ctl_log: Vec::new(),
            active: Vec::new(),
            preemptions: VecDeque::new(),
            preemptions_total: 0,
            quotas: Vec::new(),
            next_seq: 1,
            proc_summary: observability.summary,
            proc_active: observability.active,
            proc_preemptions: observability.preemptions,
            proc_summary_bytes: observability.summary_bytes as usize,
            proc_active_bytes: observability.active_bytes as usize,
            proc_preemptions_bytes: observability.preemptions_bytes as usize,
        }
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn proc_enabled(&self) -> bool {
        self.proc_summary || self.proc_active || self.proc_preemptions
    }

    fn proc_active_enabled(&self) -> bool {
        self.proc_active
    }

    fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    fn list_proc_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        if self.proc_summary {
            push_list_entry(output, "summary")?;
        }
        if self.proc_active {
            push_list_entry(output, "active")?;
            push_list_entry(output, "by-id")?;
        }
        if self.proc_preemptions {
            push_list_entry(output, "preemptions")?;
        }
        Ok(())
    }

    fn list_active_ids_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        output.clear();
        for entry in &self.active {
            push_list_entry(output, entry.id.as_str())?;
        }
        Ok(())
    }

    fn append_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        if !self.enabled {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let command = parse_lease_ctl(payload)?;
        match command {
            LeaseCtlCommand::Grant {
                id,
                subject,
                resource,
                ttl_s,
                priority,
            } => {
                if self.active_max_entries == 0 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if self.active.len() >= self.active_max_entries {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                if self.active.iter().any(|entry| entry.id == id) {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                self.active.push(LeaseEntry {
                    id,
                    subject,
                    resource,
                    ttl_s,
                    priority,
                    state: LEASE_STATE_ACTIVE,
                    seq,
                    last_request_tag: None,
                });
            }
            LeaseCtlCommand::Renew {
                id,
                ttl_s,
                priority,
            } => {
                let entry = self
                    .active
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                entry.ttl_s = ttl_s;
                entry.priority = priority;
                entry.seq = seq;
            }
            LeaseCtlCommand::RenewBound {
                id,
                subject,
                resource,
                request,
                ttl_s,
                priority,
            } => {
                let entry = self
                    .active
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                if entry.subject != subject || entry.resource != resource {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                if entry.last_request_tag == Some(request) {
                    return if entry.ttl_s == ttl_s && entry.priority == priority {
                        Ok(())
                    } else {
                        Err(NineDoorBridgeError::InvalidPayload)
                    };
                }
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                entry.ttl_s = ttl_s;
                entry.priority = priority;
                entry.seq = seq;
                entry.last_request_tag = Some(request);
            }
            LeaseCtlCommand::Preempt { id, reason } => {
                let position = self
                    .active
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                if self.preemptions_max_entries == 0 {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let entry = self.active.swap_remove(position);
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                if self.preemptions.len() == self.preemptions_max_entries {
                    let _ = self.preemptions.pop_front();
                }
                self.preemptions.push_back(LeasePreemption {
                    id: entry.id,
                    subject: entry.subject,
                    resource: entry.resource,
                    reason,
                    seq,
                });
                self.preemptions_total = self.preemptions_total.saturating_add(1);
            }
            LeaseCtlCommand::Quota {
                subject,
                resource,
                max_active,
                max_preemptions,
            } => {
                if self.active_max_entries == 0 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let quota_len = self.quotas.len();
                let existing = self
                    .quotas
                    .iter_mut()
                    .find(|entry| entry.subject == subject && entry.resource == resource);
                if existing.is_none() && quota_len >= self.active_max_entries {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                if let Some(entry) = existing {
                    entry.max_active = max_active;
                    entry.max_preemptions = max_preemptions;
                } else {
                    self.quotas.push(LeaseQuota {
                        subject,
                        resource,
                        max_active,
                        max_preemptions,
                    });
                }
            }
        }
        Ok(())
    }

    fn summary_lines(
        &self,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        self.summary_lines_into(&mut output)?;
        Ok(output)
    }

    fn summary_lines_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "active={} preemptions={} quotas={} max_active={} max_preemptions={}",
            self.active.len(),
            self.preemptions_total,
            self.quotas.len(),
            self.active_max_entries,
            self.preemptions_max_entries
        );
        if line.len() > self.proc_summary_bytes || line.len() > OBSERVE_LEASE_SUMMARY_BYTES {
            return Err(NineDoorBridgeError::BufferFull);
        }
        output.clear();
        lines_from_text_into(line.as_str(), output)
    }

    fn active_lines(
        &self,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        self.active_lines_into(&mut output)?;
        Ok(output)
    }

    fn active_lines_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        output.clear();
        let mut used_bytes = 0usize;
        for entry in &self.active {
            let line = Self::active_line(entry)?;
            if !push_newline_accounted_line(output, line, &mut used_bytes, self.proc_active_bytes)?
            {
                break;
            }
        }
        Ok(())
    }

    fn active_line_into(
        &self,
        id: &str,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        output.clear();
        let Some(entry) = self.active.iter().find(|entry| entry.id == id) else {
            return Ok(());
        };
        let line = Self::active_line(entry)?;
        let mut used_bytes = 0usize;
        if !push_newline_accounted_line(output, line, &mut used_bytes, self.proc_active_bytes)? {
            return Err(NineDoorBridgeError::BufferFull);
        }
        Ok(())
    }

    fn active_line(
        entry: &LeaseEntry,
    ) -> Result<HeaplessString<DEFAULT_LINE_CAPACITY>, NineDoorBridgeError> {
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        write!(
            line,
            "id={} subject={} resource={} ttl_s={} priority={} state={} seq={}",
            entry.id,
            entry.subject,
            entry.resource,
            entry.ttl_s,
            entry.priority,
            entry.state,
            entry.seq
        )
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
        if let Some(request) = entry.last_request_tag {
            write!(line, " request={}", hex::encode(request))
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        Ok(line)
    }

    fn preemptions_lines(
        &self,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        self.preemptions_lines_into(&mut output)?;
        Ok(output)
    }

    fn preemptions_lines_into(
        &self,
        output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    ) -> Result<(), NineDoorBridgeError> {
        output.clear();
        let mut used_bytes = 0usize;
        for entry in &self.preemptions {
            let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
            write!(
                line,
                "id={} subject={} resource={} reason={} seq={}",
                entry.id, entry.subject, entry.resource, entry.reason, entry.seq
            )
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
            if !push_newline_accounted_line(
                output,
                line,
                &mut used_bytes,
                self.proc_preemptions_bytes,
            )? {
                break;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ExportWindow {
    id: String,
    ttl_s: u32,
    seq: u64,
}

#[derive(Debug)]
struct ExportState {
    enabled: bool,
    ctl_max_bytes: u32,
    ctl_log: Vec<u8>,
    windows: Vec<ExportWindow>,
    next_seq: u64,
}

impl ExportState {
    fn new(control: generated::ExportControlConfig) -> Self {
        Self {
            enabled: control.enable,
            ctl_max_bytes: control.ctl_max_bytes,
            ctl_log: Vec::new(),
            windows: Vec::new(),
            next_seq: 1,
        }
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    fn append_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        if !self.enabled {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        let command = parse_export_ctl(payload)?;
        match command {
            ExportCtlCommand::Open { id, ttl_s } => {
                let existing = self.windows.iter().position(|entry| entry.id == id);
                if existing.is_none() && self.windows.len() >= MAX_STREAM_LINES {
                    return Err(NineDoorBridgeError::BufferFull);
                }
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                if let Some(index) = existing {
                    let entry = &mut self.windows[index];
                    entry.ttl_s = ttl_s;
                    entry.seq = seq;
                } else {
                    self.windows.push(ExportWindow { id, ttl_s, seq });
                }
            }
            ExportCtlCommand::Close { id, reason: _ } => {
                let position = self
                    .windows
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                append_log_bytes(&mut self.ctl_log, payload, self.ctl_max_bytes)?;
                let _ = self.windows.swap_remove(position);
            }
        }
        Ok(())
    }
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
        write!(line, ",\"role\":\"{}\",\"ticket\":\"{}\"}}", role, ticket)
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
            PolicyGateAllowance::Action { id, target } => {
                (Some(id.as_str()), Some(target.as_str()))
            }
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

    fn append_decisions(
        &mut self,
        payload: &[u8],
    ) -> Result<AuditAppendOutcome, NineDoorBridgeError> {
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
    Stale {
        requested: u64,
        available_start: u64,
    },
    Future {
        requested: u64,
        available_end: u64,
    },
    TooManyEntries {
        requested: usize,
        max: usize,
    },
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
            ReplayWindowError::TooManyEntries { requested, max } => {
                format!("replay exceeds max entries {} > {}", requested, max)
            }
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
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_lifecycle: crate::worker_supervisor::WorkerLifecycleState,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_identity: Option<WorkerIdentity>,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_revision: u64,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_ready_sequence: u64,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_receipt_sequence: u64,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_completion_sequence: u64,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    target_published: bool,
}

#[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
impl WorkerTelemetry {
    fn apply_target_snapshot(
        &mut self,
        snapshot: TargetWorkerNamespaceSnapshot,
    ) -> Result<(), NineDoorBridgeError> {
        if snapshot.public_id() != Some(self.id.as_str())
            || snapshot.role != self.target.worker_role()
            || snapshot.revision < self.target_revision
        {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if snapshot.revision == self.target_revision {
            return Ok(());
        }
        let identity = snapshot
            .identity
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        let line = render_worker_runtime_state_v2(
            self.id.as_str(),
            target_worker_role_label(snapshot.role),
            snapshot.lifecycle.label(),
            identity,
            [
                snapshot.ready_sequence,
                snapshot.control_sequence,
                snapshot.receipt_sequence,
                snapshot.completion_sequence,
            ],
        )?;
        self.ring
            .append(line.as_bytes())
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        self.target_lifecycle = snapshot.lifecycle;
        self.target_identity = Some(identity);
        self.target_revision = snapshot.revision;
        self.target_ready_sequence = snapshot.ready_sequence;
        self.target_receipt_sequence = snapshot.receipt_sequence;
        self.target_completion_sequence = snapshot.completion_sequence;
        if snapshot.ready_sequence != 0 {
            self.target_published = true;
        }
        Ok(())
    }
}

fn render_worker_runtime_state_v2(
    worker_id: &str,
    role: &str,
    state: &str,
    identity: WorkerIdentity,
    sequences: [u64; 4],
) -> Result<HeaplessString<DEFAULT_LINE_CAPACITY>, NineDoorBridgeError> {
    let lease_epoch =
        u32::try_from(identity.lease_epoch).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let supervisor_generation = u32::try_from(identity.supervisor_generation)
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let cap_generation =
        u32::try_from(identity.cap_generation).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let ready_sequence =
        u32::try_from(sequences[0]).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let control_sequence =
        u32::try_from(sequences[1]).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let receipt_sequence =
        u32::try_from(sequences[2]).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let completion_sequence =
        u32::try_from(sequences[3]).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let mut line = HeaplessString::new();
    write!(
        line,
        "{{\"schema\":\"worker-runtime-state/v2\",\"worker_id\":\"{worker_id}\",\"role\":\"{role}\",\"state\":\"{state}\",\"identity\":[{},{},{},{}],\"sequence\":[{},{},{},{}]}}\n",
        identity.slot,
        lease_epoch,
        supervisor_generation,
        cap_generation,
        ready_sequence,
        control_sequence,
        receipt_sequence,
        completion_sequence,
    )
    .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SpawnTarget {
    Heartbeat,
    Gpu,
    Lora,
}

impl SpawnTarget {
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    const fn worker_role(self) -> WorkerRole {
        match self {
            Self::Heartbeat => WorkerRole::Heartbeat,
            Self::Gpu => WorkerRole::Gpu,
            Self::Lora => WorkerRole::Lora,
        }
    }
}

#[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
const fn target_worker_role_label(role: WorkerRole) -> &'static str {
    match role {
        WorkerRole::Heartbeat => "worker-heartbeat",
        WorkerRole::Gpu => "worker-gpu",
        WorkerRole::Lora => "worker-lora",
    }
}

#[derive(Debug)]
enum QueenCtlCommand<'a> {
    Spawn(SpawnTarget),
    Kill(&'a str),
    Bind { from: &'a str, to: &'a str },
    Mount { service: &'a str, at: &'a str },
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
struct TelemetryRead {
    start_offset: u64,
    consumed_bytes: usize,
    bytes: Vec<u8>,
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

    fn read_from(&self, offset: u64, max_bytes: usize) -> TelemetryRead {
        let mut start_offset = offset.max(self.base_offset);
        if start_offset > self.next_offset {
            start_offset = self.next_offset;
        }
        let available = self.next_offset.saturating_sub(start_offset) as usize;
        let read_len = available.min(max_bytes);
        let mut bytes = Vec::with_capacity(read_len);
        if read_len > 0 {
            let capacity = self.capacity.max(1);
            let start_idx = (start_offset % capacity as u64) as usize;
            let first_len = (capacity - start_idx).min(read_len);
            bytes.extend_from_slice(&self.buffer[start_idx..start_idx + first_len]);
            if first_len < read_len {
                let remaining = read_len - first_len;
                bytes.extend_from_slice(&self.buffer[..remaining]);
            }
        }
        TelemetryRead {
            start_offset,
            consumed_bytes: read_len,
            bytes,
        }
    }
}

fn host_provider_label(provider: generated::HostProvider) -> &'static str {
    match provider {
        generated::HostProvider::Systemd => "systemd",
        generated::HostProvider::K8s => "k8s",
        generated::HostProvider::Docker => "docker",
        generated::HostProvider::Nvidia => "nvidia",
        generated::HostProvider::Jetson => "jetson",
        generated::HostProvider::Net => "net",
    }
}

fn host_ticket_action_label(action: generated::HostTicketAction) -> &'static str {
    match action {
        generated::HostTicketAction::GpuLeaseGrant => "gpu.lease.grant",
        generated::HostTicketAction::GpuLeaseRenew => "gpu.lease.renew",
        generated::HostTicketAction::GpuLeaseRelease => "gpu.lease.release",
        generated::HostTicketAction::PeftExport => "peft.export",
        generated::HostTicketAction::PeftImport => "peft.import",
        generated::HostTicketAction::PeftActivate => "peft.activate",
        generated::HostTicketAction::PeftRollback => "peft.rollback",
        generated::HostTicketAction::SystemdStart => "systemd.start",
        generated::HostTicketAction::SystemdStop => "systemd.stop",
        generated::HostTicketAction::SystemdRestart => "systemd.restart",
        generated::HostTicketAction::SystemdStatusCheck => "systemd.status-check",
        generated::HostTicketAction::DockerRestart => "docker.restart",
        generated::HostTicketAction::DockerStop => "docker.stop",
        generated::HostTicketAction::DockerStatusCheck => "docker.status-check",
        generated::HostTicketAction::K8sCordon => "k8s.cordon",
        generated::HostTicketAction::K8sDrain => "k8s.drain",
        generated::HostTicketAction::K8sLeaseSync => "k8s.lease.sync",
    }
}

fn host_ticket_lifecycle_label(state: generated::HostTicketLifecycleState) -> &'static str {
    match state {
        generated::HostTicketLifecycleState::Queued => "queued",
        generated::HostTicketLifecycleState::Claimed => "claimed",
        generated::HostTicketLifecycleState::Running => "running",
        generated::HostTicketLifecycleState::Succeeded => "succeeded",
        generated::HostTicketLifecycleState::Failed => "failed",
        generated::HostTicketLifecycleState::Expired => "expired",
    }
}

fn validate_host_ticket_token(value: &str) -> Result<(), NineDoorBridgeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if trimmed.len() > HOST_TICKET_ID_MAX_BYTES {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':'))
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_host_ticket_v2_token(value: &str) -> Result<(), NineDoorBridgeError> {
    validate_host_ticket_token(value)?;
    if value.starts_with('-') {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn parse_host_ticket_v2_spec(
    line: &str,
    host: &HostState,
) -> Result<HostTicketV2RawSpec, NineDoorBridgeError> {
    let spec: HostTicketV2RawSpec =
        serde_json::from_str(line).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    if spec.schema != HOST_TICKET_V2_REQUEST_SCHEMA
        || !host.accepted_request_schema(spec.schema.as_str())
        || spec.receipt_mode != "worker"
        || !host.receipt_action(spec.action.as_str())
        || !host
            .ticket_action_allowlist
            .iter()
            .any(|allowed| host_ticket_action_label(*allowed) == spec.action)
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    validate_host_ticket_v2_token(spec.id.as_str())?;
    validate_host_ticket_v2_token(spec.idempotency_key.as_str())?;
    validate_host_ticket_v2_token(spec.operation_id.as_str())?;
    validate_host_ticket_v2_token(spec.subject_ref.as_str())?;
    validate_host_ticket_v2_token(spec.receipt_worker_role.as_str())?;
    validate_host_ticket_v2_token(spec.receipt_worker_id.as_str())?;
    if spec.receipt_worker_id.len() > HOST_TICKET_WORKER_ID_MAX_BYTES
        || spec.receipt_supervisor_generation == 0
        || spec.receipt_cap_generation == 0
        || spec.expires_unix_ms == Some(0)
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let action = host_ticket_action(spec.action.as_str())?;
    if spec.receipt_worker_role != host_ticket_worker_role_label(action.role()) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    validate_host_ticket_v2_args(action, &spec.args)?;
    Ok(spec)
}

fn parse_host_ticket_v2_result(
    line: &str,
    host: &HostState,
) -> Result<HostTicketV2Result, NineDoorBridgeError> {
    let result: HostTicketV2Result =
        serde_json::from_str(line).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    if result.schema != HOST_TICKET_V2_RESULT_SCHEMA
        || !host.accepted_result_schema(result.schema.as_str())
        || result.receipt_mode != "worker"
        || !host.receipt_action(result.action.as_str())
        || !host
            .ticket_action_allowlist
            .iter()
            .any(|allowed| host_ticket_action_label(*allowed) == result.action)
        || !host
            .ticket_lifecycle
            .iter()
            .any(|allowed| host_ticket_lifecycle_label(*allowed) == result.state)
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    validate_host_ticket_v2_token(result.id.as_str())?;
    validate_host_ticket_v2_token(result.idempotency_key.as_str())?;
    validate_host_ticket_v2_token(result.operation_id.as_str())?;
    validate_host_ticket_v2_token(result.subject_ref.as_str())?;
    validate_host_ticket_v2_token(result.receipt_worker_role.as_str())?;
    validate_host_ticket_v2_token(result.receipt_worker_id.as_str())?;
    if result.receipt_worker_id.len() > HOST_TICKET_WORKER_ID_MAX_BYTES
        || result.receipt_supervisor_generation == 0
        || result.receipt_cap_generation == 0
        || result.resolved_lease_epoch == 0
        || result.admission_sequence == 0
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if result.message.as_deref().is_some_and(|message| {
        message.is_empty() || message.len() > 192 || message.chars().any(char::is_control)
    }) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let action = host_ticket_action(result.action.as_str())?;
    if result.receipt_worker_role != host_ticket_worker_role_label(action.role()) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let supplied = decode_sha256(result.result_digest.as_str())?;
    let canonical = canonical_host_ticket_v2_result_bytes(&result)?;
    if supplied != sha256_bytes(canonical.as_slice()) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(result)
}

fn validate_host_ticket_v2_subject(
    raw: &HostTicketV2RawSpec,
    gpu: &GpuState,
) -> Result<(), NineDoorBridgeError> {
    if host_ticket_action(raw.action.as_str())?.is_gpu()
        && gpu.entry(raw.subject_ref.as_str()).is_none()
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_host_ticket_v2_args(
    action: WorkerAction,
    args: &JsonValue,
) -> Result<(), NineDoorBridgeError> {
    let object = args
        .as_object()
        .ok_or(NineDoorBridgeError::InvalidPayload)?;
    match action {
        WorkerAction::GpuLeaseGrant | WorkerAction::GpuLeaseRenew => {
            validate_host_ticket_arg_keys(object, &["priority", "ttl_s"])?;
            validate_host_ticket_optional_u64(object, "ttl_s", 1, u32::MAX as u64)?;
            validate_host_ticket_optional_u64(object, "priority", 0, u8::MAX as u64)
        }
        WorkerAction::GpuLeaseRelease => {
            validate_host_ticket_arg_keys(object, &["reason"])?;
            if let Some(reason) = object.get("reason") {
                let reason = reason.as_str().ok_or(NineDoorBridgeError::InvalidPayload)?;
                if reason.is_empty()
                    || reason.len() > HOST_TICKET_REASON_MAX_BYTES
                    || reason.chars().any(char::is_control)
                {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
            Ok(())
        }
        WorkerAction::PeftExport | WorkerAction::PeftActivate | WorkerAction::PeftRollback => {
            validate_host_ticket_arg_keys(object, &[])
        }
        WorkerAction::PeftImport => {
            validate_host_ticket_arg_keys(
                object,
                &[
                    "adapter_ref",
                    "adapter_sha256",
                    "job_id",
                    "lora_sha256",
                    "metrics_sha256",
                ],
            )?;
            for key in ["adapter_ref", "job_id"] {
                let value = object
                    .get(key)
                    .and_then(JsonValue::as_str)
                    .ok_or(NineDoorBridgeError::InvalidPayload)?;
                validate_host_ticket_v2_token(value)?;
            }
            for key in ["adapter_sha256", "lora_sha256", "metrics_sha256"] {
                if let Some(value) = object.get(key) {
                    let value = value.as_str().ok_or(NineDoorBridgeError::InvalidPayload)?;
                    if value.len() != 64
                        || value.bytes().any(|byte| {
                            !byte.is_ascii_hexdigit()
                                || (byte.is_ascii_alphabetic() && byte.is_ascii_uppercase())
                        })
                    {
                        return Err(NineDoorBridgeError::InvalidPayload);
                    }
                }
            }
            Ok(())
        }
        WorkerAction::HeartbeatPublish => Err(NineDoorBridgeError::InvalidPayload),
    }
}

fn validate_host_ticket_arg_keys(
    object: &JsonMap<String, JsonValue>,
    allowed: &[&str],
) -> Result<(), NineDoorBridgeError> {
    if object
        .keys()
        .any(|key| !allowed.iter().any(|allowed| *allowed == key))
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_host_ticket_optional_u64(
    object: &JsonMap<String, JsonValue>,
    key: &str,
    minimum: u64,
    maximum: u64,
) -> Result<(), NineDoorBridgeError> {
    let Some(value) = object.get(key) else {
        return Ok(());
    };
    let value = value.as_u64().ok_or(NineDoorBridgeError::InvalidPayload)?;
    if !(minimum..=maximum).contains(&value) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn admit_host_ticket_v2_spec(
    raw: HostTicketV2RawSpec,
    binding: HostTicketV2WorkerBinding<'_>,
    admission_sequence: u64,
) -> Result<HostTicketV2AdmittedSpec, NineDoorBridgeError> {
    let action = host_ticket_action(raw.action.as_str())?;
    if admission_sequence == 0
        || !binding.ready
        || binding.ready_sequence == 0
        || binding.public_id != raw.receipt_worker_id
        || binding.role != action.role()
        || binding.identity.validate_for_role(binding.role).is_err()
        || binding.identity.supervisor_generation != raw.receipt_supervisor_generation
        || binding.identity.cap_generation != raw.receipt_cap_generation
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let resolved_worker_slot =
        u16::try_from(binding.identity.slot).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    Ok(HostTicketV2AdmittedSpec {
        schema: raw.schema,
        id: raw.id,
        idempotency_key: raw.idempotency_key,
        action: raw.action,
        args: raw.args,
        expires_unix_ms: raw.expires_unix_ms,
        receipt_mode: raw.receipt_mode,
        operation_id: raw.operation_id,
        subject_ref: raw.subject_ref,
        receipt_worker_role: raw.receipt_worker_role,
        receipt_worker_id: raw.receipt_worker_id,
        receipt_supervisor_generation: raw.receipt_supervisor_generation,
        receipt_cap_generation: raw.receipt_cap_generation,
        resolved_worker_slot,
        resolved_lease_epoch: binding.identity.lease_epoch,
        admission_sequence,
    })
}

fn validate_result_binding(
    result: &HostTicketV2Result,
    admitted: &HostTicketV2AdmittedSpec,
) -> Result<(), NineDoorBridgeError> {
    if result.id != admitted.id
        || result.idempotency_key != admitted.idempotency_key
        || result.action != admitted.action
        || result.receipt_mode != admitted.receipt_mode
        || result.operation_id != admitted.operation_id
        || result.subject_ref != admitted.subject_ref
        || result.receipt_worker_role != admitted.receipt_worker_role
        || result.receipt_worker_id != admitted.receipt_worker_id
        || result.receipt_supervisor_generation != admitted.receipt_supervisor_generation
        || result.receipt_cap_generation != admitted.receipt_cap_generation
        || result.resolved_worker_slot != admitted.resolved_worker_slot
        || result.resolved_lease_epoch != admitted.resolved_lease_epoch
        || result.admission_sequence != admitted.admission_sequence
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn host_ticket_action(action: &str) -> Result<WorkerAction, NineDoorBridgeError> {
    match action {
        "gpu.lease.grant" => Ok(WorkerAction::GpuLeaseGrant),
        "gpu.lease.renew" => Ok(WorkerAction::GpuLeaseRenew),
        "gpu.lease.release" => Ok(WorkerAction::GpuLeaseRelease),
        "peft.export" => Ok(WorkerAction::PeftExport),
        "peft.import" => Ok(WorkerAction::PeftImport),
        "peft.activate" => Ok(WorkerAction::PeftActivate),
        "peft.rollback" => Ok(WorkerAction::PeftRollback),
        _ => Err(NineDoorBridgeError::InvalidPayload),
    }
}

const fn host_ticket_worker_role_label(role: WorkerRole) -> &'static str {
    match role {
        WorkerRole::Heartbeat => "worker-heartbeat",
        WorkerRole::Gpu => "worker-gpu",
        WorkerRole::Lora => "worker-lora",
    }
}

fn host_ticket_terminal_outcome(state: &str) -> Result<Option<WorkerOutcome>, NineDoorBridgeError> {
    match state {
        "queued" | "claimed" | "running" => Ok(None),
        "succeeded" => Ok(Some(WorkerOutcome::Confirmed)),
        "failed" | "expired" => Ok(Some(WorkerOutcome::Rejected)),
        _ => Err(NineDoorBridgeError::InvalidPayload),
    }
}

fn host_ticket_terminal_disposition(
    outcome: WorkerOutcome,
    admitted: &HostTicketV2AdmittedSpec,
    current: Option<HostTicketV2WorkerBinding<'_>>,
) -> Result<HostTicketV2TerminalDisposition, NineDoorBridgeError> {
    let expected_role = host_ticket_action(admitted.action.as_str())?.role();
    let admitted_identity = admission_identity(admitted)?;
    let exact_ready = current.is_some_and(|binding| {
        binding.ready
            && binding.ready_sequence != 0
            && binding.public_id == admitted.receipt_worker_id
            && binding.role == expected_role
            && binding.identity == admitted_identity
    });
    Ok(if exact_ready {
        HostTicketV2TerminalDisposition::Submit(outcome)
    } else {
        HostTicketV2TerminalDisposition::Stale
    })
}

fn admission_identity(
    spec: &HostTicketV2AdmittedSpec,
) -> Result<WorkerIdentity, NineDoorBridgeError> {
    let role = host_ticket_action(spec.action.as_str())?.role();
    let identity = WorkerIdentity::new(
        role,
        u32::from(spec.resolved_worker_slot),
        spec.resolved_lease_epoch,
        spec.receipt_supervisor_generation,
        spec.receipt_cap_generation,
    );
    identity
        .validate_for_role(role)
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    Ok(identity)
}

fn next_host_ticket_admission_sequence(current: u64) -> Result<(u64, u64), NineDoorBridgeError> {
    if current == 0 {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let next = current.checked_add(1).ok_or(NineDoorBridgeError::Busy)?;
    Ok((current, next))
}

/// Select one deterministic terminal admission to retire when the bounded
/// exact-current/idempotency window is full. Nonterminal admissions retain
/// their Worker reservation and are never candidates; saturation by active
/// work therefore remains an explicit `busy` result.
fn host_ticket_admission_retirement_candidate(
    admissions: &BTreeMap<[u8; 32], HostTicketV2Admission>,
) -> Result<Option<[u8; 32]>, NineDoorBridgeError> {
    if admissions.len() < HOST_TICKET_MAX_ADMISSIONS {
        return Ok(None);
    }
    if admissions.len() != HOST_TICKET_MAX_ADMISSIONS {
        return Err(NineDoorBridgeError::Busy);
    }
    admissions
        .iter()
        .filter(|(_, admission)| {
            admission.terminal_result_digest.is_some() && admission.terminal_outcome.is_some()
        })
        .min_by(|(left_digest, left), (right_digest, right)| {
            left.spec
                .admission_sequence
                .cmp(&right.spec.admission_sequence)
                .then_with(|| left_digest.cmp(right_digest))
        })
        .map(|(digest, _)| Some(*digest))
        .ok_or(NineDoorBridgeError::Busy)
}

fn ensure_host_ticket_worker_available(
    admissions: &BTreeMap<[u8; 32], HostTicketV2Admission>,
    binding: HostTicketV2WorkerBinding<'_>,
) -> Result<(), NineDoorBridgeError> {
    if binding.current_control_sequence != 0 {
        return Err(NineDoorBridgeError::Busy);
    }
    for admission in admissions.values() {
        if admission_identity(&admission.spec).ok() != Some(binding.identity)
            || admission.spec.receipt_worker_id != binding.public_id
        {
            continue;
        }
        if admission.terminal_result_digest.is_none() {
            return Err(NineDoorBridgeError::Busy);
        }
    }
    Ok(())
}

fn next_host_ticket_worker_control_sequence(
    binding: HostTicketV2WorkerBinding<'_>,
) -> Result<u64, NineDoorBridgeError> {
    if binding.current_control_sequence != 0 {
        return Err(NineDoorBridgeError::Busy);
    }
    binding
        .last_control_sequence
        .checked_add(1)
        .ok_or(NineDoorBridgeError::Busy)
}

#[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
fn host_ticket_current_line(
    admission: &HostTicketV2Admission,
    current: Option<&TargetWorkerNamespaceSnapshot>,
) -> Result<HeaplessString<DEFAULT_LINE_CAPACITY>, NineDoorBridgeError> {
    let state = match admission.terminal_outcome {
        None => "pending",
        Some(WorkerOutcome::Confirmed) => "confirmed",
        Some(WorkerOutcome::Rejected) => "rejected",
        Some(WorkerOutcome::Stale) => "stale",
        Some(WorkerOutcome::NotApplicable) => return Err(NineDoorBridgeError::InvalidPayload),
    };
    let expected_identity = admission_identity(&admission.spec)?;
    let exact = current.filter(|snapshot| {
        snapshot.identity == Some(expected_identity)
            && snapshot.public_id() == Some(admission.spec.receipt_worker_id.as_str())
    });
    let lifecycle = exact.map_or("absent", |snapshot| snapshot.lifecycle.label());
    let (ready, control, receipt, completion) = exact.map_or((0, 0, 0, 0), |snapshot| {
        (
            snapshot.ready_sequence,
            snapshot.control_sequence,
            snapshot.receipt_sequence,
            snapshot.completion_sequence,
        )
    });
    let mut line = HeaplessString::new();
    write!(
        line,
        "HOST_TICKET_CURRENT schema={} state={} role={} worker={} lifecycle={} identity={},{},{},{} sequence={},{},{},{} admission={}",
        HOST_TICKET_CURRENT_SCHEMA,
        state,
        admission.spec.receipt_worker_role,
        admission.spec.receipt_worker_id,
        lifecycle,
        expected_identity.slot,
        expected_identity.lease_epoch,
        expected_identity.supervisor_generation,
        expected_identity.cap_generation,
        ready,
        control,
        receipt,
        completion,
        admission.spec.admission_sequence,
    )
    .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn build_host_ticket_worker_control(
    result: &HostTicketV2Result,
    outcome: WorkerOutcome,
    admitted_time_ns: u64,
    worker_control_sequence: u64,
) -> Result<WorkerControlRecord, NineDoorBridgeError> {
    let action = host_ticket_action(result.action.as_str())?;
    let identity = WorkerIdentity::new(
        action.role(),
        u32::from(result.resolved_worker_slot),
        result.resolved_lease_epoch,
        result.receipt_supervisor_generation,
        result.receipt_cap_generation,
    );
    identity
        .validate_for_role(action.role())
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let result_digest = decode_sha256(result.result_digest.as_str())?;
    Ok(WorkerControlRecord::staged(
        worker_control_sequence,
        identity,
        action,
        outcome,
        admitted_time_ns,
        0,
        ReceiptDigests {
            ticket: Digest32::new(sha256_bytes(result.id.as_bytes())),
            idempotency: Digest32::new(sha256_bytes(result.idempotency_key.as_bytes())),
            operation: Digest32::new(sha256_bytes(result.operation_id.as_bytes())),
            subject: Digest32::new(sha256_bytes(result.subject_ref.as_bytes())),
            result: Digest32::new(result_digest),
        },
    )
    .committed())
}

#[derive(Serialize)]
struct HostTicketV2CanonicalResult<'a> {
    schema: &'a str,
    id: &'a str,
    idempotency_key: &'a str,
    action: &'a str,
    state: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    receipt_mode: &'a str,
    operation_id: &'a str,
    subject_ref: &'a str,
    receipt_worker_role: &'a str,
    receipt_worker_id: &'a str,
    receipt_supervisor_generation: u64,
    receipt_cap_generation: u64,
    resolved_worker_slot: u16,
    resolved_lease_epoch: u64,
    admission_sequence: u64,
}

fn canonical_host_ticket_v2_result_bytes(
    result: &HostTicketV2Result,
) -> Result<Vec<u8>, NineDoorBridgeError> {
    serde_json::to_vec(&HostTicketV2CanonicalResult {
        schema: result.schema.as_str(),
        id: result.id.as_str(),
        idempotency_key: result.idempotency_key.as_str(),
        action: result.action.as_str(),
        state: result.state.as_str(),
        message: result.message.as_deref(),
        receipt_mode: result.receipt_mode.as_str(),
        operation_id: result.operation_id.as_str(),
        subject_ref: result.subject_ref.as_str(),
        receipt_worker_role: result.receipt_worker_role.as_str(),
        receipt_worker_id: result.receipt_worker_id.as_str(),
        receipt_supervisor_generation: result.receipt_supervisor_generation,
        receipt_cap_generation: result.receipt_cap_generation,
        resolved_worker_slot: result.resolved_worker_slot,
        resolved_lease_epoch: result.resolved_lease_epoch,
        admission_sequence: result.admission_sequence,
    })
    .map_err(|_| NineDoorBridgeError::InvalidPayload)
}

fn serialize_host_ticket<T: Serialize>(value: &T) -> Result<String, NineDoorBridgeError> {
    serde_json::to_string(value).map_err(|_| NineDoorBridgeError::InvalidPayload)
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut output = [0u8; 32];
    output.copy_from_slice(&digest);
    output
}

fn host_ticket_correlation_digest(
    id: &str,
    idempotency_key: &str,
) -> Result<[u8; 32], NineDoorBridgeError> {
    let id_len = u16::try_from(id.len()).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let key_len =
        u16::try_from(idempotency_key.len()).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let mut hasher = Sha256::new();
    hasher.update(b"host-ticket-correlation/v1\0");
    hasher.update(id_len.to_be_bytes());
    hasher.update(id.as_bytes());
    hasher.update(key_len.to_be_bytes());
    hasher.update(idempotency_key.as_bytes());
    let digest = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&digest);
    Ok(output)
}

fn parse_host_ticket_current_path(path: &str) -> Result<Option<[u8; 32]>, NineDoorBridgeError> {
    let Some(encoded) = path.strip_prefix(HOST_TICKET_CURRENT_PREFIX) else {
        return Ok(None);
    };
    if encoded.is_empty() || encoded.contains('/') {
        return Err(NineDoorBridgeError::InvalidPath);
    }
    decode_sha256(encoded)
        .map(Some)
        .map_err(|_| NineDoorBridgeError::InvalidPath)
}

fn parse_proc_lease_by_id_path(path: &str) -> Result<Option<&str>, NineDoorBridgeError> {
    let Some(id) = path.strip_prefix(PROC_LEASE_BY_ID_PREFIX) else {
        return Ok(None);
    };
    if id.is_empty() || id.contains('/') || validate_lease_id(id).is_err() {
        return Err(NineDoorBridgeError::InvalidPath);
    }
    Ok(Some(id))
}

fn decode_sha256(value: &str) -> Result<[u8; 32], NineDoorBridgeError> {
    if value.len() != 64
        || value.bytes().any(|byte| {
            !byte.is_ascii_hexdigit() || (byte.is_ascii_alphabetic() && byte.is_ascii_uppercase())
        })
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let mut output = [0u8; 32];
    hex::decode_to_slice(value, &mut output).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    Ok(output)
}

fn cat_wire_line_count(text: &str) -> Result<usize, NineDoorBridgeError> {
    let mut total = 0usize;
    for line in text.lines() {
        let line_count = if line.len() <= DEFAULT_LINE_CAPACITY {
            1
        } else {
            cat_chunk_count(line)?
        };
        total = total
            .checked_add(line_count)
            .ok_or(NineDoorBridgeError::BufferFull)?;
    }
    Ok(total)
}

fn cat_chunk_count(line: &str) -> Result<usize, NineDoorBridgeError> {
    let mut count = 0usize;
    let mut start = 0usize;
    while start < line.len() {
        start = cat_chunk_end(line, start)?;
        count = count
            .checked_add(1)
            .ok_or(NineDoorBridgeError::BufferFull)?;
    }
    if count == 0 || count > MAX_STREAM_LINES || count > u16::MAX as usize {
        return Err(NineDoorBridgeError::BufferFull);
    }
    Ok(count)
}

fn cat_chunk_end(line: &str, start: usize) -> Result<usize, NineDoorBridgeError> {
    if start >= line.len() || !line.is_char_boundary(start) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let mut end = start.saturating_add(CAT_CHUNK_TEXT_BYTES).min(line.len());
    while end > start && !line.is_char_boundary(end) {
        end -= 1;
    }
    if end == start {
        return Err(NineDoorBridgeError::BufferFull);
    }
    Ok(end)
}

fn cat_lines_from_text_into(
    text: &str,
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<(), NineDoorBridgeError> {
    output.clear();
    for line in text.lines() {
        if line.len() <= DEFAULT_LINE_CAPACITY {
            push_boot_line(output, line)?;
            continue;
        }
        let count = cat_chunk_count(line)?;
        let digest = hex::encode(sha256_bytes(line.as_bytes()));
        let mut start = 0usize;
        let mut sequence = 0usize;
        while start < line.len() {
            let end = cat_chunk_end(line, start)?;
            let mut wire = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
            write!(
                wire,
                "{CAT_CHUNK_PREFIX}{sequence:04x}:{count:04x}:{digest}:{}",
                &line[start..end]
            )
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
            output
                .push(wire)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
            sequence = sequence
                .checked_add(1)
                .ok_or(NineDoorBridgeError::BufferFull)?;
            start = end;
        }
    }
    Ok(())
}

fn bounded_text_tail_into(
    text: &str,
    base_offset: u64,
    cursor_offset: u64,
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<TelemetryTailMeta, NineDoorBridgeError> {
    let retained_end = base_offset
        .checked_add(text.len() as u64)
        .ok_or(NineDoorBridgeError::BufferFull)?;
    let start_offset = cursor_offset.max(base_offset).min(retained_end);
    let start = usize::try_from(start_offset.saturating_sub(base_offset))
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    if !text.is_char_boundary(start) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let unread = &text[start..];
    let mut consumed_bytes = 0usize;
    let mut wire_lines = 0usize;
    for segment in unread.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let next_lines = if line.len() <= DEFAULT_LINE_CAPACITY {
            1
        } else {
            cat_chunk_count(line)?
        };
        if consumed_bytes.saturating_add(segment.len()) > UI_MAX_STREAM_BYTES
            || wire_lines.saturating_add(next_lines) > MAX_STREAM_LINES
        {
            break;
        }
        consumed_bytes = consumed_bytes
            .checked_add(segment.len())
            .ok_or(NineDoorBridgeError::BufferFull)?;
        wire_lines = wire_lines
            .checked_add(next_lines)
            .ok_or(NineDoorBridgeError::BufferFull)?;
    }
    if consumed_bytes == 0 && !unread.is_empty() {
        return Err(NineDoorBridgeError::BufferFull);
    }
    cat_lines_from_text_into(&unread[..consumed_bytes], output)?;
    Ok(TelemetryTailMeta {
        start_offset,
        consumed_bytes,
    })
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
    generated::shard_labels()
        .iter()
        .any(|entry| *entry == label)
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
    list_from_slice_into(entries, &mut output)?;
    Ok(output)
}

fn list_from_slice_into(
    entries: &[&str],
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<(), NineDoorBridgeError> {
    for entry in entries {
        push_list_entry(output, entry)?;
    }
    Ok(())
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

fn push_newline_accounted_line(
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    line: HeaplessString<DEFAULT_LINE_CAPACITY>,
    used_bytes: &mut usize,
    max_bytes: usize,
) -> Result<bool, NineDoorBridgeError> {
    let serialized_len = line.len().saturating_add(1);
    if serialized_len > DEFAULT_LINE_CAPACITY {
        return Err(NineDoorBridgeError::BufferFull);
    }
    if used_bytes.saturating_add(serialized_len) > max_bytes {
        return Ok(false);
    }
    output
        .push(line)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    *used_bytes = used_bytes.saturating_add(serialized_len);
    Ok(true)
}

fn lines_from_bytes_into(
    data: &[u8],
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<(), NineDoorBridgeError> {
    let text = str::from_utf8(data).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    lines_from_text_into(text, output)
}

fn lines_from_bytes(
    data: &[u8],
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    lines_from_bytes_into(data, &mut output)?;
    Ok(output)
}

fn cas_lines_from_bytes_into(
    data: &[u8],
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<(), NineDoorBridgeError> {
    output.clear();
    let encoded = BASE64_STANDARD.encode(data);
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
        return Ok(());
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
    Ok(())
}

fn cas_lines_from_bytes(
    data: &[u8],
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    cas_lines_from_bytes_into(data, &mut output)?;
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

fn lines_from_text_into(
    text: &str,
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<(), NineDoorBridgeError> {
    script_lines_into(text, output)
}

fn lines_from_text(
    text: &str,
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    lines_from_text_into(text, &mut output)?;
    Ok(output)
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
        snapshot.epoch, snapshot.state
    );
    let _ = writeln!(
        text,
        "manifest_bytes={} manifest_pending_bytes={}",
        snapshot.manifest_bytes, snapshot.manifest_pending_bytes
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
        snapshot.payload_bytes, payload_sha
    );
    let _ = writeln!(
        text,
        "delta_base_epoch={} delta_base_sha256={}",
        delta_epoch, delta_sha
    );
    ensure_ui_stream_len(text.len())?;
    Ok(text.into_bytes())
}

fn build_update_status_cbor(
    snapshot: &UpdateStatusSnapshot,
) -> Result<Vec<u8>, NineDoorBridgeError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer.map(11).map_err(cbor_error)?;
    writer
        .text("epoch")
        .and_then(|_| writer.text(snapshot.epoch.as_str()))
        .map_err(cbor_error)?;
    writer
        .text("state")
        .and_then(|_| writer.text(snapshot.state))
        .map_err(cbor_error)?;
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
        .and_then(
            |_| match (&snapshot.delta_base_epoch, snapshot.delta_base_sha256) {
                (Some(epoch), Some(sha)) => {
                    writer.map(2)?;
                    writer.text("base_epoch")?;
                    writer.text(epoch.as_str())?;
                    writer.text("base_sha256")?;
                    writer.bytes(&sha)?;
                    Ok(())
                }
                _ => writer.null(),
            },
        )
        .map_err(cbor_error)?;
    Ok(writer.finish())
}

fn render_p50_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_P50_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "p50_ms={}", snapshot.p50_ms).map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_p95_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_P95_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "p95_ms={}", snapshot.p95_ms).map_err(|_| NineDoorBridgeError::BufferFull)?;
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
    write!(line, "dropped={}", snapshot.dropped).map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_queued_line(
    snapshot: IngestSnapshot,
) -> Result<HeaplessString<OBSERVE_QUEUED_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "queued={}", snapshot.queued).map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_root_reachable_line(
    snapshot: lifecycle::RootSnapshot,
) -> Result<HeaplessString<OBSERVE_ROOT_REACHABLE_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    let value = if snapshot.reachable { "yes" } else { "no" };
    write!(line, "reachable={value}").map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_root_last_seen_line(
    snapshot: lifecycle::RootSnapshot,
) -> Result<HeaplessString<OBSERVE_ROOT_LAST_SEEN_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "last_seen_ms={}", snapshot.last_seen_ms)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_root_cut_reason_line(
    snapshot: lifecycle::RootSnapshot,
) -> Result<HeaplessString<OBSERVE_ROOT_CUT_REASON_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    let reason = lifecycle::root_cut_reason_label(snapshot.cut_reason);
    write!(line, "cut_reason={reason}").map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_pressure_busy_line(
    snapshot: crate::observe::PressureSnapshot,
) -> Result<HeaplessString<OBSERVE_PRESSURE_BUSY_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "busy={}", snapshot.busy).map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_pressure_quota_line(
    snapshot: crate::observe::PressureSnapshot,
) -> Result<HeaplessString<OBSERVE_PRESSURE_QUOTA_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "quota={}", snapshot.quota).map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_pressure_cut_line(
    snapshot: crate::observe::PressureSnapshot,
) -> Result<HeaplessString<OBSERVE_PRESSURE_CUT_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "cut={}", snapshot.cut).map_err(|_| NineDoorBridgeError::BufferFull)?;
    Ok(line)
}

fn render_pressure_policy_line(
    snapshot: crate::observe::PressureSnapshot,
) -> Result<HeaplessString<OBSERVE_PRESSURE_POLICY_BYTES>, NineDoorBridgeError> {
    let mut line = HeaplessString::new();
    write!(line, "policy={}", snapshot.policy).map_err(|_| NineDoorBridgeError::BufferFull)?;
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
        delay_ms, OBSERVE_WATCH_MIN_INTERVAL_MS
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

fn validate_json_keys(payload: &str, allowed: &[&str]) -> Result<(), NineDoorBridgeError> {
    let trimmed = payload.trim();
    validate_json_envelope(trimmed)?;
    let bytes = trimmed.as_bytes();
    let mut depth = 0usize;
    let mut idx = 0usize;
    let mut seen: Vec<&str> = Vec::new();
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => {
                depth = depth.saturating_add(1);
                if depth > 1 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
            }
            b'}' => {
                if depth == 0 {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                depth = depth.saturating_sub(1);
            }
            b'[' | b']' => {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            b'"' => {
                let start = idx + 1;
                let mut end = start;
                let mut escape = false;
                while end < bytes.len() {
                    let byte = bytes[end];
                    if escape {
                        escape = false;
                        end = end.saturating_add(1);
                        continue;
                    }
                    if byte == b'\\' {
                        escape = true;
                        end = end.saturating_add(1);
                        continue;
                    }
                    if byte == b'"' {
                        break;
                    }
                    end = end.saturating_add(1);
                }
                if end >= bytes.len() {
                    return Err(NineDoorBridgeError::InvalidPayload);
                }
                let key = &trimmed[start..end];
                let prev = prev_non_ws(bytes, idx);
                let next = next_non_ws(bytes, end + 1);
                if depth == 1 && matches!(prev, Some(b'{') | Some(b',')) && next == Some(b':') {
                    if !allowed.iter().any(|entry| *entry == key) {
                        return Err(NineDoorBridgeError::InvalidPayload);
                    }
                    if seen.iter().any(|entry| *entry == key) {
                        return Err(NineDoorBridgeError::InvalidPayload);
                    }
                    seen.push(key);
                }
                idx = end.saturating_add(1);
                continue;
            }
            _ => {}
        }
        idx = idx.saturating_add(1);
    }
    if depth != 0 {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn prev_non_ws(bytes: &[u8], idx: usize) -> Option<u8> {
    if idx == 0 {
        return None;
    }
    let mut pos = idx;
    while pos > 0 {
        pos -= 1;
        let byte = bytes[pos];
        if !byte.is_ascii_whitespace() {
            return Some(byte);
        }
    }
    None
}

fn next_non_ws(bytes: &[u8], idx: usize) -> Option<u8> {
    let mut pos = idx;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if !byte.is_ascii_whitespace() {
            return Some(byte);
        }
        pos += 1;
    }
    None
}

#[derive(Debug)]
struct ScheduleRequest {
    id: String,
    role: String,
    priority: u32,
    ticks: u32,
    budget_ms: u32,
}

#[derive(Debug)]
enum ScheduleCtlCommand {
    Enqueue(ScheduleRequest),
    Dequeue { id: String },
}

#[derive(Debug)]
enum LeaseCtlCommand {
    Grant {
        id: String,
        subject: String,
        resource: String,
        ttl_s: u32,
        priority: u32,
    },
    Renew {
        id: String,
        ttl_s: u32,
        priority: u32,
    },
    RenewBound {
        id: String,
        subject: String,
        resource: String,
        request: [u8; LEASE_REQUEST_TAG_BYTES],
        ttl_s: u32,
        priority: u32,
    },
    Preempt {
        id: String,
        reason: String,
    },
    Quota {
        subject: String,
        resource: String,
        max_active: u32,
        max_preemptions: u32,
    },
}

#[derive(Debug)]
enum ExportCtlCommand {
    Open { id: String, ttl_s: u32 },
    Close { id: String, reason: String },
}

#[derive(Debug)]
enum PolicyCtlCommand {
    Apply { id: String, sha256: String },
    Rollback { id: String },
}

fn parse_schedule_ctl(payload: &str) -> Result<ScheduleCtlCommand, NineDoorBridgeError> {
    if let Some(op) = parse_json_string_field(payload, "op") {
        if op != "dequeue" {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        validate_json_keys(payload, &["op", "id"])?;
        let id =
            parse_json_string_field(payload, "id").ok_or(NineDoorBridgeError::InvalidPayload)?;
        validate_schedule_id(id)?;
        return Ok(ScheduleCtlCommand::Dequeue { id: id.to_owned() });
    }
    validate_json_keys(payload, &["id", "role", "priority", "ticks", "budget_ms"])?;
    let id = parse_json_string_field(payload, "id").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let role =
        parse_json_string_field(payload, "role").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let priority =
        parse_json_u64_field(payload, "priority").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let ticks =
        parse_json_u64_field(payload, "ticks").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let budget_ms =
        parse_json_u64_field(payload, "budget_ms").ok_or(NineDoorBridgeError::InvalidPayload)?;
    validate_schedule_id(id)?;
    validate_schedule_role(role)?;
    let priority = u32::try_from(priority).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let ticks = u32::try_from(ticks).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    let budget_ms = u32::try_from(budget_ms).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    if ticks == 0 || budget_ms == 0 {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(ScheduleCtlCommand::Enqueue(ScheduleRequest {
        id: id.to_owned(),
        role: role.to_owned(),
        priority,
        ticks,
        budget_ms,
    }))
}

fn parse_lease_ctl(payload: &str) -> Result<LeaseCtlCommand, NineDoorBridgeError> {
    let op = parse_json_string_field(payload, "op").ok_or(NineDoorBridgeError::InvalidPayload)?;
    match op {
        "grant" => {
            validate_json_keys(
                payload,
                &["op", "id", "subject", "resource", "ttl_s", "priority"],
            )?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let subject = parse_json_string_field(payload, "subject")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let resource = parse_json_string_field(payload, "resource")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let ttl_s = parse_json_u64_field(payload, "ttl_s")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let priority = parse_json_u64_field(payload, "priority")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_lease_id(id)?;
            validate_lease_subject(subject)?;
            validate_lease_resource(resource)?;
            let ttl_s = u32::try_from(ttl_s).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            let priority =
                u32::try_from(priority).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            if ttl_s == 0 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            Ok(LeaseCtlCommand::Grant {
                id: id.to_owned(),
                subject: subject.to_owned(),
                resource: resource.to_owned(),
                ttl_s,
                priority,
            })
        }
        "renew" => {
            validate_json_keys(payload, &["op", "id", "ttl_s", "priority"])?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let ttl_s = parse_json_u64_field(payload, "ttl_s")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let priority = parse_json_u64_field(payload, "priority")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_lease_id(id)?;
            let ttl_s = u32::try_from(ttl_s).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            let priority =
                u32::try_from(priority).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            if ttl_s == 0 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            Ok(LeaseCtlCommand::Renew {
                id: id.to_owned(),
                ttl_s,
                priority,
            })
        }
        "renew-bound" => {
            validate_json_keys(
                payload,
                &[
                    "op", "id", "subject", "resource", "request", "ttl_s", "priority",
                ],
            )?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let subject = parse_json_string_field(payload, "subject")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let resource = parse_json_string_field(payload, "resource")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let request = parse_json_string_field(payload, "request")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let ttl_s = parse_json_u64_field(payload, "ttl_s")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let priority = parse_json_u64_field(payload, "priority")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_lease_id(id)?;
            validate_lease_subject(subject)?;
            validate_lease_resource(resource)?;
            let request = decode_lease_request_tag(request)?;
            let ttl_s = u32::try_from(ttl_s).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            let priority =
                u32::try_from(priority).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            if ttl_s == 0 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            Ok(LeaseCtlCommand::RenewBound {
                id: id.to_owned(),
                subject: subject.to_owned(),
                resource: resource.to_owned(),
                request,
                ttl_s,
                priority,
            })
        }
        "preempt" => {
            validate_json_keys(payload, &["op", "id", "reason"])?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let reason = parse_json_string_field(payload, "reason")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_lease_id(id)?;
            validate_lease_reason(reason)?;
            Ok(LeaseCtlCommand::Preempt {
                id: id.to_owned(),
                reason: reason.to_owned(),
            })
        }
        "quota" => {
            validate_json_keys(
                payload,
                &["op", "subject", "resource", "max_active", "max_preemptions"],
            )?;
            let subject = parse_json_string_field(payload, "subject")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let resource = parse_json_string_field(payload, "resource")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let max_active = parse_json_u64_field(payload, "max_active")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let max_preemptions = parse_json_u64_field(payload, "max_preemptions")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_lease_subject(subject)?;
            validate_lease_resource(resource)?;
            let max_active =
                u32::try_from(max_active).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            let max_preemptions =
                u32::try_from(max_preemptions).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            if max_active == 0 || max_preemptions == 0 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            Ok(LeaseCtlCommand::Quota {
                subject: subject.to_owned(),
                resource: resource.to_owned(),
                max_active,
                max_preemptions,
            })
        }
        _ => Err(NineDoorBridgeError::InvalidPayload),
    }
}

fn parse_export_ctl(payload: &str) -> Result<ExportCtlCommand, NineDoorBridgeError> {
    let op = parse_json_string_field(payload, "op").ok_or(NineDoorBridgeError::InvalidPayload)?;
    match op {
        "open" => {
            validate_json_keys(payload, &["op", "id", "ttl_s"])?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let ttl_s = parse_json_u64_field(payload, "ttl_s")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_export_id(id)?;
            let ttl_s = u32::try_from(ttl_s).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
            if ttl_s == 0 {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            Ok(ExportCtlCommand::Open {
                id: id.to_owned(),
                ttl_s,
            })
        }
        "close" => {
            validate_json_keys(payload, &["op", "id", "reason"])?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let reason = parse_json_string_field(payload, "reason")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_export_id(id)?;
            validate_lease_reason(reason)?;
            Ok(ExportCtlCommand::Close {
                id: id.to_owned(),
                reason: reason.to_owned(),
            })
        }
        _ => Err(NineDoorBridgeError::InvalidPayload),
    }
}

fn parse_policy_ctl(payload: &str) -> Result<PolicyCtlCommand, NineDoorBridgeError> {
    let op = parse_json_string_field(payload, "op").ok_or(NineDoorBridgeError::InvalidPayload)?;
    match op {
        "apply" => {
            validate_json_keys(payload, &["op", "id", "sha256"])?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let sha256 = parse_json_string_field(payload, "sha256")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_policy_revision_id(id)?;
            validate_sha256_hex(sha256)?;
            Ok(PolicyCtlCommand::Apply {
                id: id.to_owned(),
                sha256: sha256.to_owned(),
            })
        }
        "rollback" => {
            validate_json_keys(payload, &["op", "id"])?;
            let id = parse_json_string_field(payload, "id")
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            validate_policy_revision_id(id)?;
            Ok(PolicyCtlCommand::Rollback { id: id.to_owned() })
        }
        _ => Err(NineDoorBridgeError::InvalidPayload),
    }
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
    let decision =
        parse_json_string_field(line, "decision").ok_or(NineDoorBridgeError::InvalidPayload)?;
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

fn validate_simple_token(value: &str, max_len: usize) -> Result<(), NineDoorBridgeError> {
    if value.is_empty() || value.len() > max_len {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_extended_token(value: &str, max_len: usize) -> Result<(), NineDoorBridgeError> {
    if value.is_empty() || value.len() > max_len {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if value == "." || value == ".." {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ':')
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn validate_schedule_id(id: &str) -> Result<(), NineDoorBridgeError> {
    validate_simple_token(id, MAX_SCHEDULE_ID_LEN)
}

fn validate_schedule_role(role: &str) -> Result<(), NineDoorBridgeError> {
    validate_simple_token(role, MAX_SCHEDULE_ROLE_LEN)
}

fn validate_lease_id(id: &str) -> Result<(), NineDoorBridgeError> {
    validate_simple_token(id, MAX_LEASE_ID_LEN)
}

fn validate_lease_subject(subject: &str) -> Result<(), NineDoorBridgeError> {
    validate_simple_token(subject, MAX_LEASE_SUBJECT_LEN)
}

fn validate_lease_resource(resource: &str) -> Result<(), NineDoorBridgeError> {
    validate_extended_token(resource, MAX_LEASE_RESOURCE_LEN)
}

fn validate_lease_reason(reason: &str) -> Result<(), NineDoorBridgeError> {
    validate_extended_token(reason, MAX_LEASE_REASON_LEN)
}

fn decode_lease_request_tag(
    value: &str,
) -> Result<[u8; LEASE_REQUEST_TAG_BYTES], NineDoorBridgeError> {
    if value.len() != LEASE_REQUEST_TAG_BYTES * 2
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let mut tag = [0u8; LEASE_REQUEST_TAG_BYTES];
    hex::decode_to_slice(value.as_bytes(), &mut tag)
        .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    Ok(tag)
}

fn validate_policy_revision_id(id: &str) -> Result<(), NineDoorBridgeError> {
    validate_simple_token(id, MAX_POLICY_REV_ID_LEN)
}

fn validate_export_id(id: &str) -> Result<(), NineDoorBridgeError> {
    validate_simple_token(id, MAX_EXPORT_ID_LEN)
}

fn validate_sha256_hex(value: &str) -> Result<(), NineDoorBridgeError> {
    if value.len() != 64 {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    if !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
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
    for (pattern_segment, path_segment) in pattern_segments.iter().zip(path_segments.iter()) {
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

fn telemetry_ingest_ctl_device(path: &str) -> Option<&str> {
    let segments = split_path_segments(path);
    match segments.as_slice() {
        ["queen", "telemetry", device_id, "ctl"] => Some(device_id),
        _ => None,
    }
}

fn telemetry_ingest_latest_path(path: &str) -> Option<&str> {
    let segments = split_path_segments(path);
    match segments.as_slice() {
        ["queen", "telemetry", device_id, "latest"] => Some(device_id),
        _ => None,
    }
}

fn telemetry_ingest_segment_path(path: &str) -> Option<(&str, &str)> {
    let segments = split_path_segments(path);
    match segments.as_slice() {
        ["queen", "telemetry", device_id, "seg", seg_id] => Some((device_id, seg_id)),
        _ => None,
    }
}

fn telemetry_ingest_device_root(path: &str) -> Option<&str> {
    let segments = split_path_segments(path);
    match segments.as_slice() {
        ["queen", "telemetry", device_id] => Some(device_id),
        _ => None,
    }
}

fn telemetry_ingest_seg_dir(path: &str) -> Option<&str> {
    let segments = split_path_segments(path);
    match segments.as_slice() {
        ["queen", "telemetry", device_id, "seg"] => Some(device_id),
        _ => None,
    }
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

fn append_log_bytes(
    log: &mut Vec<u8>,
    payload: &str,
    max_bytes: u32,
) -> Result<(), NineDoorBridgeError> {
    let payload_bytes = payload.as_bytes();
    let needs_newline = !payload_bytes.ends_with(b"\n");
    let extra = if needs_newline { 1 } else { 0 };
    let max_bytes = max_bytes as usize;
    let payload_len = payload_bytes.len().saturating_add(extra);
    if payload_len > max_bytes {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let new_len = log.len().saturating_add(payload_len);
    if new_len > max_bytes {
        let mut drop_len = new_len.saturating_sub(max_bytes);
        if drop_len < log.len() {
            if let Some(pos) = log[drop_len..].iter().position(|byte| *byte == b'\n') {
                drop_len = drop_len.saturating_add(pos + 1);
            } else {
                drop_len = log.len();
            }
        }
        log.drain(0..drop_len);
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
        NineDoorBridgeError::Busy => ErrorCode::Busy,
        NineDoorBridgeError::BufferFull => ErrorCode::TooBig,
        NineDoorBridgeError::InvalidPayload
        | NineDoorBridgeError::Unsupported(_)
        | NineDoorBridgeError::AttachTimeout => ErrorCode::Invalid,
    }
}

fn lifecycle_error_to_bridge(error: lifecycle::LifecycleError) -> NineDoorBridgeError {
    match error {
        lifecycle::LifecycleError::OutstandingLeases { .. } => NineDoorBridgeError::Permission,
        lifecycle::LifecycleError::InvalidCommand
        | lifecycle::LifecycleError::InvalidTransition
        | lifecycle::LifecycleError::AutoTransitionDenied => NineDoorBridgeError::InvalidPayload,
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
            "lora" => SpawnTarget::Lora,
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
    if payload.contains("\"mount\"") {
        let service = parse_json_string_field(payload, "service")
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        let at =
            parse_json_string_field(payload, "at").ok_or(NineDoorBridgeError::InvalidPayload)?;
        return Ok(QueenCtlCommand::Mount { service, at });
    }
    Err(NineDoorBridgeError::InvalidPayload)
}

fn parse_telemetry_ctl(payload: &str) -> Result<(), NineDoorBridgeError> {
    let command =
        parse_json_string_field(payload, "new").ok_or(NineDoorBridgeError::InvalidPayload)?;
    if command != "segment" {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct TelemetryReferenceChunk {
    seq: u64,
    offset: u64,
    chunk_bytes: u64,
}

fn parse_telemetry_reference_chunk(
    payload: &str,
) -> Result<Option<TelemetryReferenceChunk>, NineDoorBridgeError> {
    let Some(schema) = parse_json_string_field(payload, "schema") else {
        return Ok(None);
    };
    if schema != TELEMETRY_REFERENCE_CHUNK_SCHEMA {
        return Ok(None);
    }
    let seq = parse_json_u64_field(payload, "seq").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let offset = parse_json_u64_field(payload, "off").ok_or(NineDoorBridgeError::InvalidPayload)?;
    let chunk_bytes =
        parse_json_u64_field(payload, "len").ok_or(NineDoorBridgeError::InvalidPayload)?;
    if chunk_bytes == 0 {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    let digest =
        parse_json_string_field(payload, "sha256").ok_or(NineDoorBridgeError::InvalidPayload)?;
    if !is_valid_reference_digest(digest) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(Some(TelemetryReferenceChunk {
        seq,
        offset,
        chunk_bytes,
    }))
}

fn is_valid_reference_digest(value: &str) -> bool {
    if value.is_empty() || value.len() > TELEMETRY_REFERENCE_DIGEST_MAX_BYTES {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+' | '/' | '=' | '.'))
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

fn boot_lines_into(
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<(), NineDoorBridgeError> {
    output.clear();
    push_boot_line(output, BOOT_HEADER)?;
    // Keep the shim output concise so console ack summaries remain within bounds.
    for line in generated::initial_audit_lines() {
        if line.starts_with("manifest.schema=")
            || line.starts_with("manifest.profile=")
            || line.starts_with("manifest.sha256=")
            || line.starts_with("manifest.features.net_console=")
            || line.starts_with("manifest.hw.")
            || line.starts_with("attestation.")
            || line.starts_with("telemetry.")
            || line.starts_with("event_pump.")
        {
            push_boot_line(output, line)?;
        }
    }
    Ok(())
}

fn boot_lines(
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    boot_lines_into(&mut output)?;
    Ok(output)
}

fn script_lines_into(
    script: &str,
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
) -> Result<(), NineDoorBridgeError> {
    output.clear();
    for line in script.lines() {
        push_boot_line(output, line)?;
    }
    Ok(())
}

fn script_lines(
    script: &str,
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    script_lines_into(script, &mut output)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bootstrap::log as boot_log, event::AuditSink};
    use alloc::vec;
    use cohesix_cas::{CasManifest, CAS_MANIFEST_MAX_CHUNKS, CAS_MANIFEST_SCHEMA};
    use ed25519_dalek::{Signature, SigningKey};
    use sha2::{Digest, Sha256};
    use signature::Signer;

    #[derive(Default)]
    struct TestAudit;

    impl AuditSink for TestAudit {
        fn info(&mut self, _message: &str) {}

        fn denied(&mut self, _message: &str) {}
    }

    #[test]
    fn worker_runtime_state_v2_fits_the_fixed_console_line() {
        let line = render_worker_runtime_state_v2(
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "worker-heartbeat",
            "terminal",
            WorkerIdentity::new(
                WorkerRole::Heartbeat,
                u32::MAX,
                u64::from(u32::MAX),
                u64::from(u32::MAX),
                u64::from(u32::MAX),
            ),
            [u64::from(u32::MAX); 4],
        )
        .expect("maximum wire values fit one line");

        assert_eq!(line.len(), 243);
        assert!(line.ends_with("\n"));
        assert!(line.len() <= DEFAULT_LINE_CAPACITY);
        assert!(matches!(
            render_worker_runtime_state_v2(
                "worker-1",
                "worker-gpu",
                "ready",
                WorkerIdentity::new(WorkerRole::Gpu, 0, u64::from(u32::MAX) + 1, 1, 1,),
                [1, 0, 0, 0],
            ),
            Err(NineDoorBridgeError::InvalidPayload)
        ));
    }

    fn install_test_bus_adapter(bridge: &mut NineDoorBridge, scope: &str, mount: &str) {
        bridge.sidecars.bus.adapters.push(SidecarBusAdapterState {
            mount_root: sidecar_mount_root("/bus", mount),
            mount_label: mount.to_owned(),
            scope: scope.to_owned(),
            spool: OfflineSpool::new(SpoolConfig::new(8, 1_024)),
            link_state: LinkState::Offline,
            telemetry: Vec::new(),
            ctl: Vec::new(),
            link: Vec::new(),
            replay: Vec::new(),
        });
    }

    fn move_lifecycle_online_for_test() {
        let _ = lifecycle::init(0);
        let _ = lifecycle::auto_boot_complete(1);
    }

    fn empty_gpu_snapshot(sequence: u64, ttl_ms: u64) -> GpuBridgeSnapshot {
        GpuBridgeSnapshot {
            identity: GpuSnapshotIdentity {
                source_id: "gpu-bridge-host/nvml".to_owned(),
                source_mode: "production".to_owned(),
                epoch: 7,
                sequence,
                observed_unix_ms: 1,
                ttl_ms,
                catalog_sha256: gpu_catalog_sha256(&[]),
                available: false,
            },
            entries: Vec::new(),
            models: Vec::new(),
            active: String::new(),
            activation_generation: 0,
            activation_receipt: String::new(),
            telemetry_schema: b"{}".to_vec(),
        }
    }

    fn host_ticket_v2_request_fixture() -> String {
        concat!(
            "{\"schema\":\"host-ticket/v2\",\"id\":\"ticket-v2\",",
            "\"idempotency_key\":\"idem-v2\",\"action\":\"gpu.lease.grant\",",
            "\"args\":{\"ttl_s\":30},\"receipt_mode\":\"worker\",",
            "\"operation_id\":\"lease-1\",\"subject_ref\":\"GPU-0\",",
            "\"receipt_worker_role\":\"worker-gpu\",",
            "\"receipt_worker_id\":\"worker-gpu-1\",",
            "\"receipt_supervisor_generation\":2,\"receipt_cap_generation\":3}"
        )
        .to_owned()
    }

    fn host_ticket_v2_admitted_fixture() -> HostTicketV2AdmittedSpec {
        let host = HostState::new();
        let raw = parse_host_ticket_v2_spec(host_ticket_v2_request_fixture().as_str(), &host)
            .expect("parse v2 request fixture");
        admit_host_ticket_v2_spec(
            raw,
            HostTicketV2WorkerBinding {
                public_id: "worker-gpu-1",
                role: WorkerRole::Gpu,
                identity: WorkerIdentity::new(WorkerRole::Gpu, 0, 4, 2, 3),
                ready: true,
                ready_sequence: 1,
                current_control_sequence: 0,
                last_control_sequence: 0,
            },
            5,
        )
        .expect("admit v2 request fixture")
    }

    #[test]
    fn host_ticket_current_path_uses_bounded_correlation_digest() {
        let digest =
            host_ticket_correlation_digest("ticket-v2", "idem-v2").expect("correlation digest");
        assert_eq!(
            hex::encode(digest),
            "ce114e927e7cbec302f7c7a1d07be28c79b3602e8451636f1d0104a629ae39e8"
        );
        let path = format!("{HOST_TICKET_CURRENT_PREFIX}{}", hex::encode(digest));
        assert_eq!(
            parse_host_ticket_current_path(path.as_str()).expect("current path"),
            Some(digest)
        );
        assert!(matches!(
            parse_host_ticket_current_path("/host/tickets/current/not-a-digest"),
            Err(NineDoorBridgeError::InvalidPath)
        ));
        assert_eq!(
            parse_host_ticket_current_path("/host/tickets/status").expect("unrelated path"),
            None
        );
    }

    fn host_ticket_v2_result_fixture(
        admitted: &HostTicketV2AdmittedSpec,
        state: &str,
        message: Option<&str>,
    ) -> HostTicketV2Result {
        let mut result = HostTicketV2Result {
            schema: HOST_TICKET_V2_RESULT_SCHEMA.to_owned(),
            id: admitted.id.clone(),
            idempotency_key: admitted.idempotency_key.clone(),
            action: admitted.action.clone(),
            state: state.to_owned(),
            message: message.map(ToOwned::to_owned),
            receipt_mode: admitted.receipt_mode.clone(),
            operation_id: admitted.operation_id.clone(),
            subject_ref: admitted.subject_ref.clone(),
            receipt_worker_role: admitted.receipt_worker_role.clone(),
            receipt_worker_id: admitted.receipt_worker_id.clone(),
            receipt_supervisor_generation: admitted.receipt_supervisor_generation,
            receipt_cap_generation: admitted.receipt_cap_generation,
            resolved_worker_slot: admitted.resolved_worker_slot,
            resolved_lease_epoch: admitted.resolved_lease_epoch,
            admission_sequence: admitted.admission_sequence,
            result_digest: String::new(),
        };
        result.result_digest = hex::encode(
            canonical_host_ticket_v2_result_bytes(&result)
                .map(|bytes| sha256_bytes(bytes.as_slice()))
                .expect("canonical result digest"),
        );
        result
    }

    #[test]
    fn gpu_snapshot_rejects_stale_sequence() {
        let mut gpu = GpuState::new(true);
        gpu.apply_bridge_snapshot(empty_gpu_snapshot(2, 100))
            .expect("first snapshot");
        let err = gpu
            .apply_bridge_snapshot(empty_gpu_snapshot(2, 100))
            .expect_err("replayed snapshot must fail");
        assert!(matches!(err, NineDoorBridgeError::InvalidPayload));
    }

    #[test]
    fn gpu_fixture_snapshot_requires_explicit_qemu_evidence_gate() {
        assert!(gpu_snapshot_mode_allowed("production", false));
        assert!(!gpu_snapshot_mode_allowed("fixture", false));
        assert!(gpu_snapshot_mode_allowed("fixture", true));
        assert!(!gpu_snapshot_mode_allowed("mock", true));
        assert!(!qemu_lora_export_fixture_allowed("fixture", false, true));
        assert!(!qemu_lora_export_fixture_allowed("fixture", true, false));
        assert!(qemu_lora_export_fixture_allowed("fixture", true, true));
        assert!(!qemu_lora_export_fixture_allowed("production", true, true));
    }

    #[test]
    fn expired_gpu_snapshot_withdraws_provider_generation() {
        crate::hal::set_timebase_now_ms(10);
        let mut gpu = GpuState::new(true);
        let mut snapshot = empty_gpu_snapshot(1, 5);
        snapshot.entries.push(GpuEntry {
            id: "GPU-live".to_owned(),
            info_payload: "{}".to_owned(),
            ctl_log: Vec::new(),
            lease_log: Vec::new(),
            status_log: Vec::new(),
        });
        gpu.apply_bridge_snapshot(snapshot)
            .expect("snapshot applies");
        gpu.withdraw_expired(15);
        assert!(gpu.entries.is_empty());
        assert!(gpu.models.is_empty());
        assert!(core::str::from_utf8(&gpu.bridge.status)
            .expect("status utf8")
            .contains("reason=expired"));
    }

    #[test]
    fn host_ticket_v2_admission_is_strict_root_owned_and_stable() {
        let host = HostState::new();
        let raw_line = host_ticket_v2_request_fixture();
        let raw = parse_host_ticket_v2_spec(raw_line.as_str(), &host).expect("strict raw v2");
        let admitted = admit_host_ticket_v2_spec(
            raw,
            HostTicketV2WorkerBinding {
                public_id: "worker-gpu-1",
                role: WorkerRole::Gpu,
                identity: WorkerIdentity::new(WorkerRole::Gpu, 0, 4, 2, 3),
                ready: true,
                ready_sequence: 1,
                current_control_sequence: 0,
                last_control_sequence: 4,
            },
            5,
        )
        .expect("strict admitted v2");
        let encoded = serialize_host_ticket(&admitted).expect("canonical admitted v2");
        assert_eq!(encoded.len(), 394);
        assert_eq!(admitted.resolved_worker_slot, 0);
        assert_eq!(admitted.resolved_lease_epoch, 4);
        assert_eq!(admitted.admission_sequence, 5);
        assert!(!encoded.contains("target"));
        assert!(!encoded.contains("source_hive"));

        for forged in [
            raw_line.replace(
                "\"receipt_cap_generation\":3}",
                "\"receipt_cap_generation\":3,\"resolved_worker_slot\":0}",
            ),
            raw_line.replace(
                "\"receipt_cap_generation\":3}",
                "\"receipt_cap_generation\":3,\"source_hive\":\"hive-a\"}",
            ),
            raw_line.replace("worker-gpu", "worker-lora"),
            raw_line.replace("\"ttl_s\":30", "\"ttl_s\":30,\"path\":\"../tmp\""),
        ] {
            assert!(parse_host_ticket_v2_spec(forged.as_str(), &host).is_err());
        }
        let peft = raw_line
            .replace("gpu.lease.grant", "peft.import")
            .replace("{\"ttl_s\":30}", "{\"adapter_ref\":\"adapter-1\",\"adapter_sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"job_id\":\"job-1\"}")
            .replace("worker-gpu", "worker-lora");
        assert!(
            parse_host_ticket_v2_spec(peft.replace("adapter-1", "../adapter").as_str(), &host,)
                .is_err()
        );
        assert!(parse_host_ticket_v2_spec(
            peft.replace(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            )
            .as_str(),
            &host,
        )
        .is_err());

        let raw = parse_host_ticket_v2_spec(raw_line.as_str(), &host).expect("strict raw v2");
        let not_ready = admit_host_ticket_v2_spec(
            raw.clone(),
            HostTicketV2WorkerBinding {
                public_id: "worker-gpu-1",
                role: WorkerRole::Gpu,
                identity: WorkerIdentity::new(WorkerRole::Gpu, 0, 4, 2, 3),
                ready: false,
                ready_sequence: 1,
                current_control_sequence: 0,
                last_control_sequence: 0,
            },
            5,
        );
        assert!(not_ready.is_err());
        let unpinned = admit_host_ticket_v2_spec(
            raw,
            HostTicketV2WorkerBinding {
                public_id: "worker-gpu-1",
                role: WorkerRole::Gpu,
                identity: WorkerIdentity::new(WorkerRole::Gpu, 0, 4, 99, 3),
                ready: true,
                ready_sequence: 1,
                current_control_sequence: 0,
                last_control_sequence: 0,
            },
            5,
        );
        assert!(unpinned.is_err());
    }

    #[test]
    fn host_ticket_admissions_are_global_and_worker_controls_are_per_identity() {
        let gpu_binding = HostTicketV2WorkerBinding {
            public_id: "worker-gpu-1",
            role: WorkerRole::Gpu,
            identity: WorkerIdentity::new(WorkerRole::Gpu, 0, 4, 2, 3),
            ready: true,
            ready_sequence: 1,
            current_control_sequence: 0,
            last_control_sequence: 0,
        };
        let lora_binding = HostTicketV2WorkerBinding {
            public_id: "worker-lora-1",
            role: WorkerRole::Lora,
            identity: WorkerIdentity::new(WorkerRole::Lora, 0, 6, 4, 2),
            ready: true,
            ready_sequence: 1,
            current_control_sequence: 0,
            last_control_sequence: 0,
        };
        assert_eq!(
            next_host_ticket_admission_sequence(1).expect("first global admission"),
            (1, 2),
        );
        assert_eq!(
            next_host_ticket_admission_sequence(2).expect("second global admission"),
            (2, 3),
        );
        assert_eq!(
            next_host_ticket_worker_control_sequence(gpu_binding).expect("first GPU control"),
            1,
        );
        assert_eq!(
            next_host_ticket_worker_control_sequence(lora_binding).expect("first LoRA control"),
            1,
        );
        let mut admissions = BTreeMap::new();
        ensure_host_ticket_worker_available(&admissions, gpu_binding).expect("GPU available");
        ensure_host_ticket_worker_available(&admissions, lora_binding).expect("LoRA available");

        let host = HostState::new();
        let raw = parse_host_ticket_v2_spec(host_ticket_v2_request_fixture().as_str(), &host)
            .expect("strict GPU request");
        let first =
            admit_host_ticket_v2_spec(raw.clone(), gpu_binding, 1).expect("first global admission");
        let first_correlation =
            host_ticket_correlation_digest(first.id.as_str(), first.idempotency_key.as_str())
                .expect("first correlation digest");
        admissions.insert(
            first_correlation,
            HostTicketV2Admission {
                spec: first,
                raw_digest: [0; 32],
                terminal_result_digest: Some([1; 32]),
                terminal_outcome: Some(WorkerOutcome::Confirmed),
            },
        );
        ensure_host_ticket_worker_available(&admissions, gpu_binding)
            .expect("terminal GPU admission releases reservation");
        ensure_host_ticket_worker_available(&admissions, lora_binding)
            .expect("unrelated LoRA remains available");
        assert_eq!(
            next_host_ticket_worker_control_sequence(HostTicketV2WorkerBinding {
                last_control_sequence: 1,
                ..gpu_binding
            })
            .expect("second GPU control"),
            2,
        );

        let second =
            admit_host_ticket_v2_spec(raw, gpu_binding, 2).expect("second global admission");
        let second_correlation =
            host_ticket_correlation_digest(second.id.as_str(), second.idempotency_key.as_str())
                .expect("second correlation digest");
        admissions.insert(
            second_correlation,
            HostTicketV2Admission {
                spec: second,
                raw_digest: [2; 32],
                terminal_result_digest: None,
                terminal_outcome: None,
            },
        );
        assert!(matches!(
            ensure_host_ticket_worker_available(&admissions, gpu_binding),
            Err(NineDoorBridgeError::Busy)
        ));
        assert!(matches!(
            next_host_ticket_worker_control_sequence(HostTicketV2WorkerBinding {
                current_control_sequence: 1,
                ..gpu_binding
            }),
            Err(NineDoorBridgeError::Busy)
        ));
    }

    #[test]
    fn host_ticket_admission_window_retires_only_the_oldest_terminal_entry() {
        let template = host_ticket_v2_admitted_fixture();
        let digest_for = |index: usize| {
            let mut digest = [0u8; 32];
            digest[..8].copy_from_slice(&(index as u64).to_be_bytes());
            digest
        };
        let mut admissions = BTreeMap::new();
        for index in 0..HOST_TICKET_MAX_ADMISSIONS {
            let mut spec = template.clone();
            spec.id = format!("ticket-{index}");
            spec.idempotency_key = format!("idem-{index}");
            spec.admission_sequence = (index as u64).saturating_add(2);
            admissions.insert(
                digest_for(index),
                HostTicketV2Admission {
                    spec,
                    raw_digest: [index as u8; 32],
                    terminal_result_digest: Some([index as u8; 32]),
                    terminal_outcome: Some(WorkerOutcome::Confirmed),
                },
            );
        }

        let oldest_terminal = digest_for(173);
        admissions
            .get_mut(&oldest_terminal)
            .expect("selected terminal admission exists")
            .spec
            .admission_sequence = 1;
        assert_eq!(
            host_ticket_admission_retirement_candidate(&admissions)
                .expect("full terminal window has a retirement candidate"),
            Some(oldest_terminal),
        );

        let active = admissions
            .get_mut(&oldest_terminal)
            .expect("selected active admission exists");
        active.terminal_result_digest = None;
        active.terminal_outcome = None;
        assert_eq!(
            host_ticket_admission_retirement_candidate(&admissions)
                .expect("active oldest entry cannot block terminal retirement"),
            Some(digest_for(0)),
        );

        for admission in admissions.values_mut() {
            admission.terminal_result_digest = None;
            admission.terminal_outcome = None;
        }
        assert!(matches!(
            host_ticket_admission_retirement_candidate(&admissions),
            Err(NineDoorBridgeError::Busy)
        ));

        admissions.remove(&digest_for(0));
        assert_eq!(
            host_ticket_admission_retirement_candidate(&admissions)
                .expect("an underfull window needs no retirement"),
            None,
        );
    }

    #[test]
    fn host_ticket_v2_gpu_subject_must_exist_in_current_inventory() {
        let host = HostState::new();
        let raw = parse_host_ticket_v2_spec(host_ticket_v2_request_fixture().as_str(), &host)
            .expect("strict raw v2");
        let mut gpu = GpuState::new(true);
        assert!(validate_host_ticket_v2_subject(&raw, &gpu).is_err());
        gpu.entries.push(GpuEntry {
            id: "GPU-0".to_owned(),
            info_payload: "{}".to_owned(),
            ctl_log: Vec::new(),
            lease_log: Vec::new(),
            status_log: Vec::new(),
        });
        validate_host_ticket_v2_subject(&raw, &gpu).expect("generated GPU subject");
    }

    #[test]
    fn host_ticket_v2_result_digest_binding_and_worker_control_are_exact() {
        let host = HostState::new();
        let admitted = host_ticket_v2_admitted_fixture();
        let result = host_ticket_v2_result_fixture(&admitted, "succeeded", Some("committed"));
        let encoded = serialize_host_ticket(&result).expect("canonical result");
        assert_eq!(encoded.len(), 506);
        assert_eq!(
            result.result_digest,
            "730b822b8b3497f4ac21e3aaddf3d5f89411b95ddc05083268725dad2fb620b0"
        );
        let parsed =
            parse_host_ticket_v2_result(encoded.as_str(), &host).expect("strict result digest");
        validate_result_binding(&parsed, &admitted).expect("exact admitted binding");
        let canonical =
            canonical_host_ticket_v2_result_bytes(&parsed).expect("canonical digest preimage");
        assert_eq!(canonical.len(), 423);

        let control = build_host_ticket_worker_control(&parsed, WorkerOutcome::Confirmed, 123, 1)
            .expect("receipt control");
        assert_eq!(control.sequence, 1);
        assert_eq!(control.committed_sequence, 1);
        assert_eq!(
            control.identity,
            admission_identity(&admitted).expect("identity")
        );
        assert_eq!(
            control.worker_action().expect("action"),
            WorkerAction::GpuLeaseGrant
        );
        assert_eq!(
            control.worker_outcome().expect("outcome"),
            WorkerOutcome::Confirmed
        );
        assert_eq!(
            control.digests.result.bytes,
            decode_sha256(&result.result_digest).expect("hash")
        );
        assert_eq!(control.digests.operation.bytes, sha256_bytes(b"lease-1"));
        assert_eq!(control.digests.subject.bytes, sha256_bytes(b"GPU-0"));

        let mut tampered = parsed.clone();
        tampered.state = "failed".to_owned();
        assert!(parse_host_ticket_v2_result(
            serialize_host_ticket(&tampered)
                .expect("encode tampered result")
                .as_str(),
            &host,
        )
        .is_err());
        let mut rebound = parsed;
        rebound.receipt_cap_generation = 4;
        assert!(validate_result_binding(&rebound, &admitted).is_err());
    }

    #[test]
    fn host_ticket_v2_terminal_mapping_preserves_rejected_and_stale() {
        assert_eq!(
            host_ticket_terminal_outcome("succeeded").expect("succeeded"),
            Some(WorkerOutcome::Confirmed)
        );
        for state in ["failed", "expired"] {
            assert_eq!(
                host_ticket_terminal_outcome(state).expect("terminal rejection"),
                Some(WorkerOutcome::Rejected)
            );
        }
        assert_eq!(
            host_ticket_terminal_outcome("running").expect("nonterminal"),
            None
        );

        let admitted = host_ticket_v2_admitted_fixture();
        let exact = HostTicketV2WorkerBinding {
            public_id: "worker-gpu-1",
            role: WorkerRole::Gpu,
            identity: admission_identity(&admitted).expect("admitted identity"),
            ready: true,
            ready_sequence: 1,
            current_control_sequence: 0,
            last_control_sequence: 0,
        };
        assert_eq!(
            host_ticket_terminal_disposition(WorkerOutcome::Rejected, &admitted, Some(exact))
                .expect("current disposition"),
            HostTicketV2TerminalDisposition::Submit(WorkerOutcome::Rejected)
        );
        let changed = HostTicketV2WorkerBinding {
            identity: WorkerIdentity::new(WorkerRole::Gpu, 0, 4, 3, 3),
            ..exact
        };
        assert_eq!(
            host_ticket_terminal_disposition(WorkerOutcome::Confirmed, &admitted, Some(changed))
                .expect("late disposition"),
            HostTicketV2TerminalDisposition::Stale
        );
        assert_eq!(
            host_ticket_terminal_disposition(WorkerOutcome::Confirmed, &admitted, None)
                .expect("torn-down disposition"),
            HostTicketV2TerminalDisposition::Stale
        );
    }

    #[test]
    fn host_ticket_cat_chunking_is_bounded_utf8_safe_and_digest_complete() {
        let line = format!(
            "{{\"schema\":\"host-ticket-result/v2\",\"message\":\"{}\"}}",
            "🙂".repeat(300)
        );
        let mut output: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        cat_lines_from_text_into(line.as_str(), &mut output).expect("chunk long CAT line");
        assert!(output.len() > 1);
        let expected_digest = hex::encode(sha256_bytes(line.as_bytes()));
        let expected_count = output.len();
        let mut reconstructed = String::new();
        for (expected_sequence, wire) in output.iter().enumerate() {
            assert!(wire.len() <= DEFAULT_LINE_CAPACITY);
            let fields = wire.splitn(5, ':').collect::<Vec<_>>();
            assert_eq!(fields.len(), 5);
            assert_eq!(fields[0], "C1");
            assert_eq!(fields[1], format!("{expected_sequence:04x}"));
            assert_eq!(fields[2], format!("{expected_count:04x}"));
            assert_eq!(fields[3], expected_digest);
            reconstructed.push_str(fields[4]);
        }
        assert_eq!(reconstructed, line);
    }

    #[test]
    fn host_ticket_logs_reject_more_than_the_existing_cat_line_bound() {
        let host = HostState::new();
        let lines = (0..=MAX_STREAM_LINES)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>();
        assert!(matches!(
            host.can_append_ticket_lines("/host/tickets/status", lines.as_slice()),
            Err(NineDoorBridgeError::BufferFull)
        ));
    }

    #[test]
    fn host_ticket_logs_evict_complete_lines_with_explicit_retention_accounting() {
        let mut host = HostState::new();
        let path = "/host/tickets/status";
        let mut expected_next = 0u64;
        let mut expected_base = 0u64;
        for index in 0..70 {
            let line = format!("line-{index}");
            let bytes = (line.len() + 1) as u64;
            if index < 6 {
                expected_base = expected_base.saturating_add(bytes);
            }
            expected_next = expected_next.saturating_add(bytes);
            host.append_ticket_lines(path, &[line])
                .expect("bounded ticket append");
        }

        let entry = host
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .expect("status entry");
        assert_eq!(entry.value.lines().count(), MAX_STREAM_LINES);
        assert_eq!(entry.retained_wire_lines, MAX_STREAM_LINES);
        assert_eq!(entry.base_offset, expected_base);
        assert_eq!(entry.next_offset, expected_next);
        assert_eq!(entry.dropped_lines, 6);
        assert_eq!(entry.dropped_bytes, expected_base);
        assert!(entry.value.starts_with("line-6\n"));

        let mut retention = HeaplessVec::new();
        host.retention_lines_into(&mut retention)
            .expect("retention summary");
        assert!(retention.iter().any(|line| {
            line.contains("path=status")
                && line.contains("retained_wire_lines=64")
                && line.contains("dropped_lines=6")
        }));

        let mut tail = HeaplessVec::new();
        let meta = bounded_text_tail_into(entry.value.as_str(), entry.base_offset, 0, &mut tail)
            .expect("stale cursor resumes at retained base");
        assert_eq!(meta.start_offset, expected_base);
        assert_eq!(tail.first().map(|line| line.as_str()), Some("line-6"));
    }

    #[test]
    fn host_ticket_tail_chunks_long_lines_and_advances_exact_cursor() {
        let mut bridge = NineDoorBridge::new();
        let line = format!(
            "{{\"schema\":\"host-ticket/v2\",\"args\":\"{}\"}}\n",
            "x".repeat(500)
        );
        assert!(bridge
            .host
            .update_value("/host/tickets/spec", line.as_str()));
        let mut output = HeaplessVec::new();
        let meta = bridge
            .tail_stream_into("/host/tickets/spec", 0, &mut output)
            .expect("bounded host TAIL")
            .expect("supported host TAIL path");

        assert_eq!(meta.start_offset, 0);
        assert_eq!(meta.consumed_bytes, line.len());
        assert!(output.len() > 1);
        assert!(output.iter().all(|entry| {
            entry.starts_with(CAT_CHUNK_PREFIX) && entry.len() <= DEFAULT_LINE_CAPACITY
        }));

        let meta = bridge
            .tail_stream_into(
                "/host/tickets/spec",
                u64::try_from(line.len()).expect("cursor"),
                &mut output,
            )
            .expect("exhausted host TAIL")
            .expect("supported host TAIL path");
        assert_eq!(meta.consumed_bytes, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn telemetry_ctl_echo_returns_created_segment_id_and_preallocates() {
        move_lifecycle_online_for_test();
        let mut bridge = NineDoorBridge::new();
        bridge.attached = true;
        bridge.session_role = Some(SessionRoleLabel::Queen);

        let outcome = bridge.echo(
            "/queen/telemetry/pi4/ctl",
            r#"{"new":"segment","mime":"text/plain"}"#,
        );
        assert!(outcome.is_ok(), "create telemetry segment");
        let Ok(outcome) = outcome else {
            return;
        };

        assert_eq!(outcome.telemetry_segment_id(), Some("seg-000001"));
        let device = bridge.telemetry_ingest.device("pi4");
        assert!(device.is_some(), "telemetry device created");
        let Some(device) = device else {
            return;
        };
        let segment = device.segments.back();
        assert!(segment.is_some(), "segment created");
        let Some(segment) = segment else {
            return;
        };
        assert_eq!(segment.id, "seg-000001");
        assert_eq!(device.latest.as_deref(), Some("seg-000001"));
        assert_eq!(
            segment.data.capacity(),
            bridge.telemetry_ingest.initial_segment_capacity()
        );
        assert!(segment.data.capacity() >= TELEMETRY_INGEST_RECORD_MAX_BYTES);
    }

    #[test]
    fn telemetry_segment_echo_uses_existing_segment_buffer() {
        move_lifecycle_online_for_test();
        let mut bridge = NineDoorBridge::new();
        bridge.attached = true;
        bridge.session_role = Some(SessionRoleLabel::Queen);
        let outcome = bridge.echo(
            "/queen/telemetry/pi4/ctl",
            r#"{"new":"segment","mime":"text/plain"}"#,
        );
        assert!(outcome.is_ok(), "create telemetry segment");
        let Ok(outcome) = outcome else {
            return;
        };
        let seg_id = outcome.telemetry_segment_id();
        assert!(seg_id.is_some(), "segment id returned");
        let Some(seg_id) = seg_id else {
            return;
        };
        let seg_id = seg_id.to_owned();
        let segment_path = format!("/queen/telemetry/pi4/seg/{seg_id}");

        let append_outcome = bridge.echo(segment_path.as_str(), "sample");
        assert!(append_outcome.is_ok(), "append telemetry segment");
        let Ok(append_outcome) = append_outcome else {
            return;
        };

        assert_eq!(append_outcome, EchoOutcome::Appended);
        let device = bridge.telemetry_ingest.device("pi4");
        assert!(device.is_some(), "telemetry device exists");
        let Some(device) = device else {
            return;
        };
        let segment = device.segments.back();
        assert!(segment.is_some(), "segment exists");
        let Some(segment) = segment else {
            return;
        };
        assert_eq!(segment.bytes, "sample\n".len());
        assert_eq!(segment.data.as_slice(), b"sample\n");
    }

    #[test]
    fn attach_succeeds_when_bridge_handoff_was_not_requested() {
        boot_log::init_logger_bootstrap_only();
        boot_log::set_no_bridge_mode(false);

        let mut bridge = NineDoorBridge::new();
        let mut audit = TestAudit;
        bridge
            .attach("queen", None, &mut audit)
            .expect("attach should succeed without EP handoff");

        assert!(bridge.attached());
        assert!(bridge.is_queen());
    }

    #[test]
    fn attach_authority_survives_post_commit_audit_failure() {
        struct PanickingAudit;

        impl AuditSink for PanickingAudit {
            fn info(&mut self, _message: &str) {
                panic!("injected post-commit audit failure");
            }

            fn denied(&mut self, _message: &str) {}
        }

        let mut bridge = NineDoorBridge::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bridge
                .attach("queen", Some("ticket-1"), &mut PanickingAudit)
                .expect("namespace preparation must succeed before audit");
        }));

        assert!(result.is_err(), "test audit must interrupt diagnostics");
        assert!(bridge.attached());
        assert!(bridge.is_queen());
        assert_eq!(bridge.session_ticket.as_deref(), Some("ticket-1"));
    }

    #[test]
    fn quiet_session_retirement_moves_allocations_once_without_overwrite() {
        let mut bridge = NineDoorBridge::new();
        bridge.attached = true;
        bridge.session_role = Some(SessionRoleLabel::Queen);
        bridge.session_ticket = Some("ticket-old".to_owned());
        bridge.session_scope = Some("scope-old".to_owned());
        bridge
            .binds
            .push(BindEntry {
                from: "/old/from".to_owned(),
                to: "/old/to".to_owned(),
            })
            .unwrap();

        bridge.fence_session_authority_quiet();
        bridge.retire_session_ticket_quiet();
        bridge.retire_session_scope_quiet();
        bridge.retire_session_binds_quiet();

        assert!(!bridge.attached);
        assert!(bridge.session_role.is_none());
        assert!(bridge.session_ticket.is_none());
        assert!(bridge.session_scope.is_none());
        assert!(bridge.binds.is_empty());
        assert_eq!(bridge.retired_session_ticket.as_deref(), Some("ticket-old"));
        assert_eq!(bridge.retired_session_scope.as_deref(), Some("scope-old"));
        assert_eq!(bridge.retired_session_binds.len(), 1);
        assert_eq!(bridge.retired_session_binds[0].from, "/old/from");

        bridge.session_ticket = Some("ticket-new".to_owned());
        bridge.session_scope = Some("scope-new".to_owned());
        bridge
            .binds
            .push(BindEntry {
                from: "/new/from".to_owned(),
                to: "/new/to".to_owned(),
            })
            .unwrap();
        bridge.retire_session_ticket_quiet();
        bridge.retire_session_scope_quiet();
        bridge.retire_session_binds_quiet();

        assert_eq!(bridge.session_ticket.as_deref(), Some("ticket-new"));
        assert_eq!(bridge.session_scope.as_deref(), Some("scope-new"));
        assert_eq!(bridge.binds.len(), 1);
        assert_eq!(bridge.retired_session_ticket.as_deref(), Some("ticket-old"));
        assert_eq!(bridge.retired_session_scope.as_deref(), Some("scope-old"));
        assert_eq!(bridge.retired_session_binds[0].from, "/old/from");
    }

    #[test]
    fn worker_ids_reuse_lowest_free_slot_after_kill() {
        let mut bridge = NineDoorBridge::new();
        bridge
            .spawn_worker(SpawnTarget::Heartbeat)
            .expect("spawn worker-1");
        bridge
            .spawn_worker(SpawnTarget::Heartbeat)
            .expect("spawn worker-2");
        bridge.remove_worker("worker-1").expect("remove worker-1");
        bridge
            .spawn_worker(SpawnTarget::Heartbeat)
            .expect("reuse worker-1");

        let mut ids: Vec<&str> = bridge
            .workers
            .iter()
            .map(|worker| worker.id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["worker-1", "worker-2"]);
    }

    #[test]
    fn queen_control_parser_accepts_the_generated_lora_spawn_target() {
        let command = parse_queen_ctl(r#"{"spawn":"lora"}"#).expect("parse lora spawn");
        assert!(matches!(command, QueenCtlCommand::Spawn(SpawnTarget::Lora)));
    }

    #[test]
    fn worker_bus_session_cannot_create_an_executable_worker() {
        let mut bridge = NineDoorBridge::new();
        bridge.attached = true;
        bridge.session_role = Some(SessionRoleLabel::WorkerBus);
        assert!(matches!(
            bridge.handle_queen_ctl(r#"{"spawn":"heartbeat"}"#),
            Err(NineDoorBridgeError::Permission)
        ));
        assert!(bridge.workers.is_empty());
    }

    #[test]
    fn thousand_worker_namespace_pressure_stays_bounded_and_recent() {
        const WORKERS: usize = 1_000;

        let mut bridge = NineDoorBridge::new();
        for index in 0..WORKERS {
            bridge
                .spawn_worker(SpawnTarget::Heartbeat)
                .unwrap_or_else(|_| panic!("spawn worker {}", index + 1));
        }
        assert_eq!(bridge.workers.len(), WORKERS);

        let mut listing: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        bridge
            .list_workers_into(&mut listing)
            .expect("list worker namespace");
        assert_eq!(listing.len(), MAX_STREAM_LINES.min(WORKERS));
        assert_eq!(
            listing.last().map(|line| line.as_str()),
            Some("worker-1000")
        );

        let retained_first = WORKERS.saturating_sub(listing.len()).saturating_add(1);
        let retained_first_id = format!("worker-{retained_first}");
        assert_eq!(
            listing.first().map(|line| line.as_str()),
            Some(retained_first_id.as_str())
        );

        let worker = bridge
            .workers
            .iter_mut()
            .find(|worker| worker.id.as_str() == "worker-1000")
            .expect("latest worker exists");
        worker
            .ring
            .append(b"heartbeat 1000")
            .expect("append latest worker telemetry");
        let worker = bridge
            .workers
            .iter()
            .find(|worker| worker.id.as_str() == "worker-1000")
            .expect("latest worker exists");
        let telemetry = worker.ring.read_from(0, UI_MAX_STREAM_BYTES);
        assert!(
            core::str::from_utf8(telemetry.bytes.as_slice())
                .expect("worker telemetry utf8")
                .contains("heartbeat 1000"),
            "latest worker telemetry missing from bounded namespace"
        );
    }

    #[test]
    fn shard_discovery_is_bounded_to_active_model_workers() {
        const WORKERS: usize = 1_000;

        let mut bridge = NineDoorBridge::new();
        for _ in 0..WORKERS {
            bridge
                .spawn_worker(SpawnTarget::Heartbeat)
                .expect("spawn model worker");
        }
        let mut listing: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        bridge
            .list_worker_shards_into(&mut listing)
            .expect("list bounded active shards");

        assert!(!listing.is_empty());
        assert!(listing.len() <= MAX_STREAM_LINES);
        for (index, label) in listing.iter().enumerate() {
            assert!(!listing[..index].iter().any(|prior| prior == label));
            assert!(bridge.workers.iter().any(|worker| {
                worker_shard_label(worker.id.as_str(), generated::sharding_config()).as_str()
                    == label.as_str()
            }));
        }
    }

    #[test]
    fn cas_manifest_reupload_is_idempotent() {
        let config = generated::cas_config();
        let key_bytes = [7u8; 32];
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let mut config = config;
        config.verification_key = Some(signing_key.verifying_key().to_bytes());
        let mut cas = CasState::new(config);
        let epoch = "1";
        let payload = vec![0u8; config.chunk_bytes as usize];
        let digest = Sha256::digest(&payload);
        let mut digest_bytes = [0u8; 32];
        digest_bytes.copy_from_slice(&digest);

        cas.append_chunk(epoch, &digest_bytes, u64::MAX, &payload)
            .expect("upload chunk");

        let manifest = build_signed_manifest(
            epoch,
            config.chunk_bytes,
            &payload,
            digest_bytes,
            &key_bytes,
        );
        let manifest_cbor = manifest.encode_signed().expect("encode manifest");

        cas.append_manifest(epoch, u64::MAX, &manifest_cbor)
            .expect("upload manifest");
        cas.append_manifest(epoch, u64::MAX, &manifest_cbor)
            .expect("reupload manifest");
    }

    #[test]
    fn cas_ninth_chunk_quota_refusal_preserves_exact_eight_chunk_state() {
        const FIRST_BASE64_SEGMENT_CHARS: usize = 124;
        let config = generated::cas_config();
        let chunk_bytes = config.chunk_bytes as usize;
        let mut cas = CasState::new(config);
        let mut committed = Vec::new();
        for index in 0..CAS_MANIFEST_MAX_CHUNKS {
            let payload = vec![b'A' + index as u8; chunk_bytes];
            let digest = Sha256::digest(&payload);
            let mut digest_bytes = [0u8; 32];
            digest_bytes.copy_from_slice(&digest);
            cas.append_chunk("1", &digest_bytes, u64::MAX, &payload)
                .expect("admit chunk within manifest capacity");
            committed.push((digest_bytes, payload));
        }

        let bytes_before = cas.bytes_used;
        let chunks_before = cas.chunks.len();
        let pending_before = cas.pending_chunks.len();
        let ninth_payload = vec![b'Z'; chunk_bytes];
        let ninth_digest = Sha256::digest(&ninth_payload);
        let mut ninth_digest_bytes = [0u8; 32];
        ninth_digest_bytes.copy_from_slice(&ninth_digest);
        let ninth_encoded = BASE64_STANDARD.encode(&ninth_payload);
        assert_eq!(ninth_encoded.len(), 172);
        let ninth_first_segment = format!("b64:{}", &ninth_encoded[..FIRST_BASE64_SEGMENT_CHARS]);
        let err = cas
            .append_chunk(
                "1",
                &ninth_digest_bytes,
                u64::MAX,
                ninth_first_segment.as_bytes(),
            )
            .expect_err("first segment of ninth chunk must exceed fixed store capacity");
        assert!(matches!(err, NineDoorBridgeError::BufferFull));

        assert_eq!(cas.bytes_used, bytes_before);
        assert_eq!(cas.chunks.len(), chunks_before);
        assert_eq!(cas.pending_chunks.len(), pending_before);
        assert!(!cas.chunks.contains_key(&ninth_digest_bytes));
        for (digest, payload) in committed {
            assert_eq!(
                cas.read_chunk(&digest).expect("read committed chunk"),
                payload
            );
        }
    }

    fn build_signed_manifest(
        epoch: &str,
        chunk_bytes: u32,
        payload: &[u8],
        digest: [u8; 32],
        key_bytes: &[u8; 32],
    ) -> CasManifest {
        let payload_digest = Sha256::digest(payload);
        let mut payload_sha256 = [0u8; 32];
        payload_sha256.copy_from_slice(&payload_digest);
        let mut manifest = CasManifest {
            schema: CAS_MANIFEST_SCHEMA.to_owned(),
            epoch: epoch.to_owned(),
            chunk_bytes,
            payload_bytes: chunk_bytes as u64,
            payload_sha256,
            chunks: vec![digest],
            delta: None,
            signature: None,
        };
        let signing_key = SigningKey::from_bytes(key_bytes);
        let payload = manifest.signature_payload().expect("signing payload");
        let signature: Signature = signing_key.sign(&payload);
        manifest.signature = Some(signature.to_bytes());
        manifest
    }

    #[test]
    fn append_log_bytes_trims_to_limit() {
        let mut log = Vec::new();
        append_log_bytes(&mut log, "line1", 16).expect("append line1");
        append_log_bytes(&mut log, "line2", 16).expect("append line2");
        append_log_bytes(&mut log, "line3", 16).expect("append line3");
        let rendered = core::str::from_utf8(&log).expect("utf8 log");
        assert_eq!(rendered, "line2\nline3\n");
    }

    #[test]
    fn append_log_bytes_rejects_oversize_payload() {
        let mut log = Vec::new();
        let err = append_log_bytes(&mut log, "0123456789", 8).unwrap_err();
        assert!(matches!(err, NineDoorBridgeError::InvalidPayload));
    }

    #[test]
    fn denied_cross_scope_bus_write_appends_audit_log_line() {
        let mut bridge = NineDoorBridge::new();
        install_test_bus_adapter(&mut bridge, "bus-main", "bus-main");
        bridge.attached = true;
        bridge.session_role = Some(SessionRoleLabel::WorkerBus);
        bridge.session_scope = Some("other-bus".to_owned());

        let err = bridge
            .echo("/bus/bus-main/ctl", "denied")
            .expect_err("worker-bus must not write another bus scope");
        assert!(matches!(err, NineDoorBridgeError::Permission));

        let mut cursor = log_buffer::tail_cursor(log_buffer::LOG_SNAPSHOT_LINES);
        let mut lines: HeaplessVec<
            HeaplessString<DEFAULT_LINE_CAPACITY>,
            { log_buffer::LOG_SNAPSHOT_LINES },
        > = HeaplessVec::new();
        let _ = log_buffer::read_cursor_lines_into(&mut cursor, &mut lines);
        assert!(
            lines.iter().any(|line| {
                line.as_str()
                    .contains("sidecar-deny kind=bus scope=other-bus")
            }),
            "denied sidecar write did not append audit log line: {lines:?}"
        );
    }

    #[test]
    fn lease_proc_lines_emit_exact_state_without_temp_buffer() {
        let control = generated::LeaseControlConfig {
            enable: true,
            active_max_entries: 4,
            preemptions_max_entries: 4,
            ctl_max_bytes: 1024,
        };
        let observability = generated::ProcLeaseConfig {
            summary: true,
            active: true,
            preemptions: true,
            summary_bytes: 1024,
            active_bytes: 1024,
            preemptions_bytes: 1024,
        };
        let mut lease = LeaseState::new(control, observability);

        lease
            .append_ctl(
                r#"{"op":"grant","id":"lease-1","subject":"worker-1","resource":"gpu0","ttl_s":30,"priority":7}"#,
            )
            .expect("grant lease");

        let mut active: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        lease
            .active_lines_into(&mut active)
            .expect("render active leases");
        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].as_str(),
            "id=lease-1 subject=worker-1 resource=gpu0 ttl_s=30 priority=7 state=ACTIVE seq=1"
        );

        lease
            .append_ctl(r#"{"op":"preempt","id":"lease-1","reason":"quota"}"#)
            .expect("preempt lease");

        lease
            .active_lines_into(&mut active)
            .expect("render empty active leases");
        assert!(active.is_empty());

        let mut preemptions: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        lease
            .preemptions_lines_into(&mut preemptions)
            .expect("render preemptions");
        assert_eq!(preemptions.len(), 1);
        assert_eq!(
            preemptions[0].as_str(),
            "id=lease-1 subject=worker-1 resource=gpu0 reason=quota seq=2"
        );
    }

    #[test]
    fn lease_preemption_control_outlives_bounded_evidence_history() {
        let control = generated::LeaseControlConfig {
            enable: true,
            active_max_entries: 2,
            preemptions_max_entries: 2,
            ctl_max_bytes: 1024,
        };
        let observability = generated::ProcLeaseConfig {
            summary: true,
            active: true,
            preemptions: true,
            summary_bytes: 1024,
            active_bytes: 1024,
            preemptions_bytes: 1024,
        };
        let mut lease = LeaseState::new(control, observability);

        for index in 1..=3 {
            lease
                .append_ctl(
                    format!(
                        r#"{{"op":"grant","id":"lease-{index}","subject":"worker-{index}","resource":"gpu0","ttl_s":30,"priority":7}}"#
                    )
                    .as_str(),
                )
                .expect("grant lease");
            lease
                .append_ctl(
                    format!(r#"{{"op":"preempt","id":"lease-{index}","reason":"quota"}}"#).as_str(),
                )
                .expect("preempt lease");
        }

        assert_eq!(lease.preemptions_total, 3);
        assert_eq!(lease.preemptions.len(), 2);
        assert_eq!(lease.preemptions[0].id, "lease-2");
        assert_eq!(lease.preemptions[1].id, "lease-3");
        let summary = lease.summary_lines().expect("render lease summary");
        assert_eq!(
            summary[0].as_str(),
            "active=0 preemptions=3 quotas=0 max_active=2 max_preemptions=2"
        );
    }

    #[test]
    fn schedule_dequeue_is_fifo_bounded_and_counted() {
        let control = generated::ScheduleControlConfig {
            enable: true,
            queue_max_entries: 4,
            ctl_max_bytes: 1024,
        };
        let observability = generated::ProcScheduleConfig {
            summary: true,
            queue: true,
            summary_bytes: 1024,
            queue_bytes: 1024,
        };
        let mut schedule = ScheduleState::new(control, observability);
        schedule
            .append_ctl(
                r#"{"id":"sched-1","role":"worker-gpu","priority":2,"ticks":3,"budget_ms":120}"#,
            )
            .expect("enqueue first schedule request");
        schedule
            .append_ctl(
                r#"{"id":"sched-2","role":"worker-lora","priority":3,"ticks":4,"budget_ms":160}"#,
            )
            .expect("enqueue second schedule request");

        assert!(matches!(
            schedule.append_ctl(r#"{"op":"dequeue","id":"sched-2"}"#),
            Err(NineDoorBridgeError::InvalidPayload)
        ));
        schedule
            .append_ctl(r#"{"op":"dequeue","id":"sched-1"}"#)
            .expect("dequeue FIFO head");

        let summary = schedule.summary_lines().expect("render schedule summary");
        assert_eq!(
            summary[0].as_str(),
            "queue=1 dequeued=1 dropped=0 max_entries=4"
        );
        let queue = schedule.queue_lines().expect("render schedule queue");
        assert_eq!(queue.len(), 1);
        assert!(queue[0].starts_with("id=sched-2 "));
    }

    #[test]
    fn lease_bound_renew_is_atomic_correlated_and_idempotent() {
        let control = generated::LeaseControlConfig {
            enable: true,
            active_max_entries: 4,
            preemptions_max_entries: 4,
            ctl_max_bytes: 1024,
        };
        let observability = generated::ProcLeaseConfig {
            summary: true,
            active: true,
            preemptions: true,
            summary_bytes: 1024,
            active_bytes: 1024,
            preemptions_bytes: 1024,
        };
        let mut lease = LeaseState::new(control, observability);
        lease
            .append_ctl(
                r#"{"op":"grant","id":"lease-1","subject":"worker-1","resource":"gpu0","ttl_s":30,"priority":7}"#,
            )
            .expect("grant lease");
        let renew = r#"{"op":"renew-bound","id":"lease-1","subject":"worker-1","resource":"gpu0","request":"00112233445566778899aabbccddeeff","ttl_s":60,"priority":9}"#;
        lease.append_ctl(renew).expect("bound renew");

        let active = lease.active_lines().expect("render renewed lease");
        assert_eq!(
            active[0].as_str(),
            "id=lease-1 subject=worker-1 resource=gpu0 ttl_s=60 priority=9 state=ACTIVE seq=2 request=00112233445566778899aabbccddeeff"
        );

        let next_seq = lease.next_seq;
        let log = lease.ctl_log.clone();
        lease.append_ctl(renew).expect("exact replay");
        assert_eq!(lease.next_seq, next_seq);
        assert_eq!(lease.ctl_log, log);

        let changed_replay = r#"{"op":"renew-bound","id":"lease-1","subject":"worker-1","resource":"gpu0","request":"00112233445566778899aabbccddeeff","ttl_s":61,"priority":9}"#;
        assert!(matches!(
            lease.append_ctl(changed_replay),
            Err(NineDoorBridgeError::InvalidPayload)
        ));
        let wrong_binding = r#"{"op":"renew-bound","id":"lease-1","subject":"worker-2","resource":"gpu0","request":"ffeeddccbbaa99887766554433221100","ttl_s":60,"priority":9}"#;
        assert!(matches!(
            lease.append_ctl(wrong_binding),
            Err(NineDoorBridgeError::InvalidPayload)
        ));
        assert_eq!(lease.next_seq, next_seq);
        assert_eq!(lease.ctl_log, log);
    }

    #[test]
    fn lease_proc_lines_preserve_newline_counted_byte_budget() {
        let control = generated::LeaseControlConfig {
            enable: true,
            active_max_entries: 4,
            preemptions_max_entries: 4,
            ctl_max_bytes: 1024,
        };
        let mut observability = generated::ProcLeaseConfig {
            summary: true,
            active: true,
            preemptions: true,
            summary_bytes: 1024,
            active_bytes: 1024,
            preemptions_bytes: 1024,
        };
        let mut lease = LeaseState::new(control, observability);
        lease
            .append_ctl(
                r#"{"op":"grant","id":"lease-1","subject":"worker-1","resource":"gpu0","ttl_s":30,"priority":7}"#,
            )
            .expect("grant lease");

        let mut active: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        lease
            .active_lines_into(&mut active)
            .expect("render active leases");
        let line_len_without_newline = active[0].len();

        observability.active_bytes = line_len_without_newline as u32;
        let mut budgeted = LeaseState::new(control, observability);
        budgeted
            .append_ctl(
                r#"{"op":"grant","id":"lease-1","subject":"worker-1","resource":"gpu0","ttl_s":30,"priority":7}"#,
            )
            .expect("grant budgeted lease");
        active.clear();
        budgeted
            .active_lines_into(&mut active)
            .expect("render budgeted active leases");
        assert!(active.is_empty());
    }

    #[test]
    fn lease_by_id_lookup_finds_entries_beyond_aggregate_byte_bound() {
        let control = generated::LeaseControlConfig {
            enable: true,
            active_max_entries: 4,
            preemptions_max_entries: 4,
            ctl_max_bytes: 1024,
        };
        let mut observability = generated::ProcLeaseConfig {
            summary: true,
            active: true,
            preemptions: true,
            summary_bytes: 1024,
            active_bytes: 1024,
            preemptions_bytes: 1024,
        };
        let mut sizing = LeaseState::new(control, observability);
        sizing
            .append_ctl(
                r#"{"op":"grant","id":"lease-1","subject":"worker-1","resource":"gpu0","ttl_s":30,"priority":7}"#,
            )
            .expect("grant sizing lease");
        let sizing_lines = sizing.active_lines().expect("render sizing lease");
        observability.active_bytes = (sizing_lines[0].len() + 1) as u32;

        let mut lease = LeaseState::new(control, observability);
        for index in 1..=3 {
            lease
                .append_ctl(
                    format!(
                        r#"{{"op":"grant","id":"lease-{index}","subject":"worker-{index}","resource":"gpu0","ttl_s":30,"priority":7}}"#
                    )
                    .as_str(),
                )
                .expect("grant lease");
        }

        let active = lease.active_lines().expect("render bounded aggregate");
        assert_eq!(active.len(), 1);
        assert!(active[0].starts_with("id=lease-1 "));

        let mut exact: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        lease
            .active_line_into("lease-3", &mut exact)
            .expect("render exact lease");
        assert_eq!(exact.len(), 1);
        assert!(exact[0].starts_with("id=lease-3 subject=worker-3 "));

        lease
            .active_line_into("lease-missing", &mut exact)
            .expect("render absent exact lease");
        assert!(exact.is_empty());
        assert_eq!(
            parse_proc_lease_by_id_path("/proc/lease/by-id/lease-3")
                .expect("parse exact lease path"),
            Some("lease-3")
        );
        assert!(parse_proc_lease_by_id_path("/proc/lease/by-id/lease-3/extra").is_err());
    }

    #[test]
    fn list_into_clears_and_emits_root_prefix() {
        let mut bridge = NineDoorBridge::new();
        let mut output: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        let mut junk = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        junk.push_str("junk").expect("insert junk");
        output.push(junk).expect("push junk");

        bridge.list_into("/", &mut output).expect("list root");

        assert!(
            !output.iter().any(|line| line.as_str() == "junk"),
            "list_into should clear existing entries"
        );

        let expected_prefix = ["gpu", "kmesg", "log", "proc", "queen", "trace"];
        assert!(
            output.len() >= expected_prefix.len(),
            "root listing missing expected entries"
        );
        for (idx, entry) in expected_prefix.iter().enumerate() {
            let Some(line) = output.get(idx) else {
                panic!("missing entry {entry} at index {idx}");
            };
            assert_eq!(line.as_str(), *entry);
        }
        assert!(
            !output.iter().any(|line| line.as_str() == "lora"),
            "AI LoRA receipts must not create a root radio namespace"
        );
    }

    #[test]
    fn cat_into_clears_and_emits_boot_header() {
        let mut bridge = NineDoorBridge::new();
        let mut output: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES> =
            HeaplessVec::new();
        let mut junk = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        junk.push_str("junk").expect("insert junk");
        output.push(junk).expect("push junk");

        bridge
            .cat_into(PROC_BOOT_PATH, &mut output)
            .expect("cat boot");

        assert!(
            !output.iter().any(|line| line.as_str() == "junk"),
            "cat_into should clear existing entries"
        );
        assert_eq!(
            output.first().map(|line| line.as_str()),
            Some(BOOT_HEADER),
            "boot output should begin with header"
        );
    }

    #[cfg(feature = "release-qemu")]
    #[test]
    fn qemu_schedule_flight_recorder_is_bounded_and_discoverable() {
        let mut bridge = NineDoorBridge::new();
        let entries = bridge
            .list(PROC_SCHEDULE_ROOT_PATH)
            .expect("QEMU schedule directory");
        assert!(entries.iter().any(|entry| entry == "qemu-flight"));

        let lines = bridge
            .cat(PROC_SCHEDULE_QEMU_FLIGHT_PATH)
            .expect("QEMU flight recorder snapshot");
        assert!(lines.len() >= 3);
        assert!(lines.len() <= MAX_STREAM_LINES);
        assert!(lines[0].starts_with("QEMU_FLIGHT_SUMMARY schema=v1"));
        assert!(lines[1].starts_with("QEMU_FLIGHT_TIMING schema=v1"));
        assert!(lines[2].starts_with("QEMU_FLIGHT_EXITS schema=v1"));
    }

    #[test]
    fn action_queue_evicts_consumed_entries_before_rejecting() {
        let mut policy = PolicyState::new();
        policy.limits.queue_max_entries = 2;
        policy.actions = vec![
            PolicyAction {
                id: "old-consumed".to_owned(),
                target: "/queen/ctl".to_owned(),
                decision: PolicyDecision::Approve,
                consumed: true,
            },
            PolicyAction {
                id: "active-1".to_owned(),
                target: "/queen/ctl".to_owned(),
                decision: PolicyDecision::Approve,
                consumed: false,
            },
        ];

        policy
            .append_action_queue(
                "{\"id\":\"active-2\",\"target\":\"/queen/ctl\",\"decision\":\"approve\"}\n",
                "queen",
                "test-ticket",
            )
            .expect("append action should evict consumed entry");

        assert_eq!(policy.actions.len(), 2);
        assert!(policy
            .actions
            .iter()
            .all(|action| action.id.as_str() != "old-consumed"));
        assert!(policy
            .actions
            .iter()
            .any(|action| action.id.as_str() == "active-2"));
    }

    #[test]
    fn action_queue_rejects_when_only_unconsumed_entries_remain() {
        let mut policy = PolicyState::new();
        policy.limits.queue_max_entries = 1;
        policy.actions = vec![PolicyAction {
            id: "active-1".to_owned(),
            target: "/queen/ctl".to_owned(),
            decision: PolicyDecision::Approve,
            consumed: false,
        }];

        let err = policy
            .append_action_queue(
                "{\"id\":\"active-2\",\"target\":\"/queen/ctl\",\"decision\":\"approve\"}\n",
                "queen",
                "test-ticket",
            )
            .expect_err("queue should reject when no consumed entries can be evicted");
        assert!(matches!(err, NineDoorBridgeError::InvalidPayload));
    }

    #[test]
    fn action_queue_reuses_consumed_action_ids_on_rerun() {
        let mut policy = PolicyState::new();
        policy.actions = vec![PolicyAction {
            id: "selftest-approve-1".to_owned(),
            target: "/queen/ctl".to_owned(),
            decision: PolicyDecision::Approve,
            consumed: true,
        }];

        policy
            .append_action_queue(
                "{\"id\":\"selftest-approve-1\",\"target\":\"/queen/ctl\",\"decision\":\"approve\"}\n",
                "queen",
                "test-ticket",
            )
            .expect("consumed action id should be reusable");

        assert_eq!(policy.actions.len(), 1);
        assert_eq!(policy.actions[0].id, "selftest-approve-1");
        assert!(!policy.actions[0].consumed);
    }

    #[test]
    fn action_queue_rejects_duplicate_ids_within_same_payload() {
        let mut policy = PolicyState::new();

        let err = policy
            .append_action_queue(
                concat!(
                    "{\"id\":\"dup\",\"target\":\"/queen/ctl\",\"decision\":\"approve\"}\n",
                    "{\"id\":\"dup\",\"target\":\"/queen/ctl\",\"decision\":\"approve\"}\n"
                ),
                "queen",
                "test-ticket",
            )
            .expect_err("duplicate ids in one payload should be rejected");
        assert!(matches!(err, NineDoorBridgeError::InvalidPayload));
    }

    #[test]
    fn containment_diagnostics_render_exact_bounded_evidence_markers() {
        let fault = NineDoorContainmentDiagnostic::Fault {
            expected_generation: 7,
            observed_generation: 7,
            fault_class: FaultClass::Timeout,
            sequence: 11,
        }
        .render()
        .expect("bounded fault diagnostic");
        assert_eq!(
            fault.as_str(),
            "[ninedoor-service] generation=7 terminal-fault class=Timeout sequence=11"
        );

        let mismatch = NineDoorContainmentDiagnostic::Fault {
            expected_generation: u64::MAX,
            observed_generation: u32::MAX,
            fault_class: FaultClass::Standard,
            sequence: u64::MAX,
        }
        .render()
        .expect("bounded mismatch diagnostic");
        assert_eq!(
            mismatch.as_str(),
            "[ninedoor-service] fault generation mismatch expected=18446744073709551615 observed=4294967295"
        );

        let teardown = NineDoorContainmentDiagnostic::Teardown {
            generation: u64::MAX,
        }
        .render()
        .expect("bounded teardown diagnostic");
        assert_eq!(
            teardown.as_str(),
            "NINEDOOR_SERVICE_TEARDOWN generation=18446744073709551615 tcb_suspended=yes mappings_scrubbed=yes recovery_reply_revoked=yes capabilities_revoked=yes generation_fenced=yes state=terminal"
        );
        assert!(fault.len() < DEFAULT_LINE_CAPACITY);
        assert!(mismatch.len() < DEFAULT_LINE_CAPACITY);
        assert!(teardown.len() < DEFAULT_LINE_CAPACITY);
    }

    #[test]
    fn containment_diagnostics_commit_in_fault_then_teardown_order() {
        let fault = NineDoorContainmentDiagnostic::InvalidMailbox { generation: 9 };
        let teardown = NineDoorContainmentDiagnostic::Teardown { generation: 9 };
        let mut bridge = NineDoorBridge::new();
        bridge.pending_containment_fault_diagnostic = Some(fault);
        bridge.pending_containment_teardown_diagnostic = Some(teardown);

        assert_eq!(bridge.pending_containment_diagnostic(), Some(fault));
        bridge.commit_containment_diagnostic(teardown);
        assert_eq!(bridge.pending_containment_diagnostic(), Some(fault));
        bridge.commit_containment_diagnostic(fault);
        assert_eq!(bridge.pending_containment_diagnostic(), Some(teardown));
        bridge.commit_containment_diagnostic(teardown);
        assert_eq!(bridge.pending_containment_diagnostic(), None);
    }
}
