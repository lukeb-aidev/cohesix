// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Construct MCS critical root progress domains from compiler-owned temporal records.
// Author: Lukas Bower

//! HAL construction for the Milestone 26e critical root domains.
//!
//! The init TCB and initial SC are the real `root-control` domain because that
//! thread owns bootstrap and HAL admission.  This module does not create a
//! phantom control child.  It creates six restricted CSpaces/TCBs, binds one
//! independently configured active SC to each, and resumes only caller-supplied
//! entrypoints for fault, emergency, Worker-supervisor, and driver-supervisor
//! work plus the two bounded passive-Worker executors. All six children share
//! the root VSpace deliberately so the small
//! root-resident entrypoints and HAL-mapped private stack/IPC pages are visible;
//! their capability views remain separate and compiler-bounded.

use crate::critical_tcb::{
    generated_standard_fault_badge, mcs_extra_refills, passive_service_recovery_contract,
    service_fault_mailbox_index, validate_critical_temporal_graph, validate_worker_supervisor_wake,
    CriticalHandoff, CriticalTcbHandle, CriticalTcbInventory, CriticalTcbOrigin,
    CriticalTopologyError, FaultClass, FaultHandoffError, FaultHandoffRecord, FaultRegistration,
    FaultRegistry, FaultRegistryError, FaultRegistrySnapshot, GenerationIdentity, PublishResult,
    WorkerControlQueue, WorkerControlQueueError, WorkerControlRecord, WorkerSupervisorItem,
    CRITICAL_TCB_COUNT, DRIVER_FAULT_RECORD_CAPACITY, FAULT_REGISTRY_CAPACITY,
    SERVICE_FAULT_RECORD_CAPACITY, WORKER_CONTROL_QUEUE_CAPACITY, WORKER_FAULT_MAILBOX_CAPACITY,
};
use crate::generated::{
    self, CriticalTcbResource, TemporalExecution, TemporalTaskConfig, TemporalTaskKind,
    TimeoutPolicy,
};
use crate::sel4::{self, KernelEnv, RamFrame};
use crate::worker_supervisor::MAX_EXECUTABLE_WORKER_SLOTS;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use heapless::Vec;
use sel4_sys::{seL4_CPtr, seL4_Error, seL4_Word};
use spin::Mutex;

const RESTRICTED_CHILD_COUNT: usize = CRITICAL_TCB_COUNT - 1;
const MAX_CRITICAL_STACK_PAGES: usize = 4;

const CHILD_STANDARD_FAULT_SLOT: seL4_CPtr = 1;
const CHILD_TIMEOUT_FAULT_SLOT: seL4_CPtr = 2;
const CHILD_INBOX_SLOT: seL4_CPtr = 3;
const CHILD_REPLY_SLOT: seL4_CPtr = 4;
const CHILD_DRIVER_RELEASE_SLOT: seL4_CPtr = 6;
const CHILD_WORKER_SIGNAL_SLOT: seL4_CPtr = 7;
const CHILD_DRIVER_RELEASE_SIGNAL_SLOT: seL4_CPtr = 7;
const CHILD_DRIVER_SIGNAL_SLOT: seL4_CPtr = 8;
const CHILD_EMERGENCY_SIGNAL_SLOT: seL4_CPtr = 9;
const CHILD_SELF_CNODE_SLOT: seL4_CPtr = 10;
const CHILD_EXECUTOR_COMPLETION_SIGNAL_SLOT: seL4_CPtr = 5;

// Slots 11..14 remain reserved for future fixed critical-control lanes. Each
// admitted linked driver then owns one exact seven-capability containment row:
// TCB, command origin, command Reply, completion origin, SC, standard fault,
// and timeout fault. Seven rows exactly fill the Pi supervisor's 64-slot CNode.
const DRIVER_SUPERVISOR_RUNTIME_CAP_SLOT_BASE: seL4_CPtr = 15;
const DRIVER_SUPERVISOR_RUNTIME_CAP_STRIDE: seL4_CPtr = 7;
const DRIVER_SUPERVISOR_RUNTIME_TCB_OFFSET: seL4_CPtr = 0;
const DRIVER_SUPERVISOR_RUNTIME_COMMAND_ORIGIN_OFFSET: seL4_CPtr = 1;
const DRIVER_SUPERVISOR_RUNTIME_COMMAND_REPLY_OFFSET: seL4_CPtr = 2;
const DRIVER_SUPERVISOR_RUNTIME_COMPLETION_ORIGIN_OFFSET: seL4_CPtr = 3;
const DRIVER_SUPERVISOR_RUNTIME_SC_OFFSET: seL4_CPtr = 4;
const DRIVER_SUPERVISOR_RUNTIME_STANDARD_FAULT_OFFSET: seL4_CPtr = 5;
const DRIVER_SUPERVISOR_RUNTIME_TIMEOUT_FAULT_OFFSET: seL4_CPtr = 6;

const ROOT_CONTROL_ID: &str = "root-control";
const ROOT_FAULT_ID: &str = "root-fault";
const ROOT_EMERGENCY_ID: &str = "root-emergency";
const WORKER_SUPERVISOR_ID: &str = "root-worker-supervisor";
const DRIVER_SUPERVISOR_ID: &str = "root-driver-supervisor";
const WORKER_EXECUTOR_GPU_ID: &str = "root-worker-executor-gpu";
const WORKER_EXECUTOR_LORA_ID: &str = "root-worker-executor-lora";

/// Concrete root-resident entrypoints for the six restricted critical children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CriticalTcbEntrypoints {
    pub root_fault: usize,
    pub root_emergency: usize,
    pub worker_supervisor: usize,
    pub driver_supervisor: usize,
    pub worker_executor_gpu: usize,
    pub worker_executor_lora: usize,
}

impl CriticalTcbEntrypoints {
    /// Return the only operational entrypoint set accepted by construction.
    #[must_use]
    pub fn root_runtime() -> Self {
        Self {
            root_fault: root_fault_entry as *const () as usize,
            root_emergency: root_emergency_entry as *const () as usize,
            worker_supervisor: root_worker_supervisor_entry as *const () as usize,
            driver_supervisor: root_driver_supervisor_entry as *const () as usize,
            worker_executor_gpu: root_worker_executor_gpu_entry as *const () as usize,
            worker_executor_lora: root_worker_executor_lora_entry as *const () as usize,
        }
    }

    fn for_id(self, id: &str) -> Option<usize> {
        match id {
            ROOT_FAULT_ID => Some(self.root_fault),
            ROOT_EMERGENCY_ID => Some(self.root_emergency),
            WORKER_SUPERVISOR_ID => Some(self.worker_supervisor),
            DRIVER_SUPERVISOR_ID => Some(self.driver_supervisor),
            WORKER_EXECUTOR_GPU_ID => Some(self.worker_executor_gpu),
            WORKER_EXECUTOR_LORA_ID => Some(self.worker_executor_lora),
            _ => None,
        }
    }

    fn validate(self) -> Result<(), CriticalTcbConstructionError> {
        for entry in [
            self.root_fault,
            self.root_emergency,
            self.worker_supervisor,
            self.driver_supervisor,
            self.worker_executor_gpu,
            self.worker_executor_lora,
        ] {
            if entry == 0 || entry & 0x3 != 0 {
                return Err(CriticalTcbConstructionError::InvalidEntrypoint);
            }
        }
        if self != Self::root_runtime() {
            return Err(CriticalTcbConstructionError::InvalidEntrypoint);
        }
        Ok(())
    }
}

/// Root-held one-hot, Write-only signal caps for critical notification wakes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CriticalSignalCaps {
    /// Unbadged root-held origin used only to mint generated per-Worker wakes.
    pub worker_supervisor_origin: seL4_CPtr,
    pub worker_supervisor: seL4_CPtr,
    pub driver_supervisor: seL4_CPtr,
    pub emergency: seL4_CPtr,
    pub root_fault_release: seL4_CPtr,
    pub worker_executor_gpu: seL4_CPtr,
    pub worker_executor_lora: seL4_CPtr,
    pub worker_executor_completion_gpu: seL4_CPtr,
    pub worker_executor_completion_lora: seL4_CPtr,
}

/// Root-held endpoint/Reply objects used by the serialized fault receive lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CriticalFaultCaps {
    pub fault_endpoint: seL4_CPtr,
    pub emergency_endpoint: seL4_CPtr,
    pub root_fault_reply: seL4_CPtr,
    pub root_emergency_reply: seL4_CPtr,
}

/// Private frame capabilities retained for one restricted critical child.
pub struct CriticalChildBacking {
    pub id: &'static str,
    pub ipc_frame: seL4_CPtr,
    pub stack_frames: Vec<seL4_CPtr, MAX_CRITICAL_STACK_PAGES>,
    pub stack_bottom: usize,
    pub stack_top: usize,
}

/// Fully constructed critical-domain inventory retained by root-control.
pub struct CriticalTcbRuntime {
    pub handles: [CriticalTcbHandle; CRITICAL_TCB_COUNT],
    pub children: Vec<CriticalChildBacking, RESTRICTED_CHILD_COUNT>,
    pub signals: CriticalSignalCaps,
    pub faults: CriticalFaultCaps,
}

/// Fatal construction error; bootstrap must not continue with a partial graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriticalTcbConstructionError {
    Generated(CriticalTopologyError),
    FaultRegistry(FaultRegistryError),
    FaultHandoff(FaultHandoffError),
    MissingGeneratedRecord,
    RegistrySealed,
    RegistryNotSealed,
    RuntimeNotReady,
    InvalidEntrypoint,
    InvalidStackLayout,
    InvalidCapabilityRights,
    Sel4 {
        stage: &'static str,
        error: seL4_Error,
    },
}

/// Root-Cspace capabilities transferred into one driver-supervisor row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverSupervisorRuntimeRootCaps {
    pub tcb: seL4_CPtr,
    pub command_endpoint_origin: seL4_CPtr,
    pub command_reply: seL4_CPtr,
    pub completion_notification_origin: seL4_CPtr,
    pub sched_context: seL4_CPtr,
    pub standard_fault_endpoint: seL4_CPtr,
    pub timeout_fault_endpoint: seL4_CPtr,
}

/// Child-local capability identities consumed by driver containment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverSupervisorRuntimeLocalCaps {
    pub tcb: seL4_CPtr,
    pub command_endpoint_origin: seL4_CPtr,
    pub command_reply: seL4_CPtr,
    pub completion_notification_origin: seL4_CPtr,
    pub sched_context: seL4_CPtr,
    pub standard_fault_endpoint: seL4_CPtr,
    pub timeout_fault_endpoint: seL4_CPtr,
}

impl From<CriticalTopologyError> for CriticalTcbConstructionError {
    fn from(value: CriticalTopologyError) -> Self {
        Self::Generated(value)
    }
}

impl From<FaultRegistryError> for CriticalTcbConstructionError {
    fn from(value: FaultRegistryError) -> Self {
        Self::FaultRegistry(value)
    }
}

impl From<FaultHandoffError> for CriticalTcbConstructionError {
    fn from(value: FaultHandoffError) -> Self {
        Self::FaultHandoff(value)
    }
}

static TARGET_HANDOFF: Mutex<CriticalHandoff> = Mutex::new(CriticalHandoff::new());
static TARGET_WORKER_CONTROL: WorkerControlQueue = WorkerControlQueue::new();
static TARGET_FAULT_REGISTRY: Mutex<FaultRegistry> = Mutex::new(FaultRegistry::new());
static TARGET_FAULT_REGISTRY_SEALED: AtomicBool = AtomicBool::new(false);
static TARGET_FAULT_RECEIVER_ACTIVE: AtomicBool = AtomicBool::new(false);
static TARGET_FATAL: AtomicBool = AtomicBool::new(false);
static TARGET_FAULT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static TARGET_RECOVERED_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static TARGET_PENDING_FAULT_LABEL: AtomicU64 = AtomicU64::new(0);
static TARGET_PENDING_FAULT_BADGE: AtomicU64 = AtomicU64::new(0);
static TARGET_PENDING_FAULT_LENGTH: AtomicUsize = AtomicUsize::new(0);
static TARGET_PENDING_FAULT_MR0: AtomicU64 = AtomicU64::new(0);
static TARGET_PENDING_FAULT_MR1: AtomicU64 = AtomicU64::new(0);
static TARGET_PENDING_FAULT_VALID: AtomicBool = AtomicBool::new(false);
static TARGET_ROOT_FAULT_TURN: AtomicUsize =
    AtomicUsize::new(RootFaultCriticalTurn::PrimeReceive as usize);
static TARGET_ROOT_FAULT_CRITICAL_TASK: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_VALID: AtomicBool = AtomicBool::new(false);
static TARGET_ROOT_FAULT_SERVICE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TARGET_ROOT_FAULT_SERVICE_TASK: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_IDENTITY_SLOT: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_IDENTITY_LEASE: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_IDENTITY_SUPERVISOR: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_IDENTITY_CAP: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_BADGE: AtomicU64 = AtomicU64::new(0);
static TARGET_ROOT_FAULT_SERVICE_CLASS: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_LABEL: AtomicU64 = AtomicU64::new(0);
static TARGET_ROOT_FAULT_SERVICE_LENGTH: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_MR0: AtomicU64 = AtomicU64::new(0);
static TARGET_ROOT_FAULT_SERVICE_MR1: AtomicU64 = AtomicU64::new(0);
static TARGET_ROOT_FAULT_SERVICE_ROOT_TCB: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_HANDLER_TCB: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_SERVICE_RECOVER_PASSIVE: AtomicBool = AtomicBool::new(false);
static DRIVER_FAULT_REPLY_BUSY: AtomicBool = AtomicBool::new(false);
static TARGET_FAULT_ENDPOINT: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_CNODE: AtomicUsize = AtomicUsize::new(0);
static TARGET_DRIVER_SUPERVISOR_CNODE: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_TCB_CAP_SLOTS: [AtomicUsize; FAULT_REGISTRY_CAPACITY] =
    [const { AtomicUsize::new(0) }; FAULT_REGISTRY_CAPACITY];
static TARGET_ROOT_CONTROL_TEMPORAL_ACTIVE: AtomicBool = AtomicBool::new(false);
static TARGET_SERVICE_RECOVERY_SLOTS: [AtomicUsize; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicUsize::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_SERVICE_RECOVERY_STATES: [AtomicUsize; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicUsize::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_SERVICE_CALL_SEQUENCES: [AtomicU64; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicU64::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_SERVICE_RECOVERY_REQUEST_SEQUENCES: [AtomicU64; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicU64::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_SERVICE_RECOVERY_FAULT_SEQUENCES: [AtomicU64; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicU64::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_SERVICE_RECOVERY_FAULT_CLASSES: [AtomicUsize; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicUsize::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_WORKER_RECOVERY_SLOTS: [AtomicUsize; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicUsize::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_RECOVERY_STATES: [AtomicUsize; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicUsize::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_CALL_SEQUENCES: [AtomicU64; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicU64::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_CALL_ROLE_SLOTS: [AtomicUsize; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicUsize::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_CALL_SUPERVISOR_GENERATIONS: [AtomicU64; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicU64::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_CALL_CAP_GENERATIONS: [AtomicU64; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicU64::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_RECOVERY_REQUEST_SEQUENCES: [AtomicU64; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicU64::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_RECOVERY_FAULT_SEQUENCES: [AtomicU64; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicU64::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];
static TARGET_WORKER_RECOVERY_FAULT_CLASSES: [AtomicUsize; MAX_EXECUTABLE_WORKER_SLOTS] =
    [const { AtomicUsize::new(0) }; MAX_EXECUTABLE_WORKER_SLOTS];

/// Coherent, copied live MCS state used by bounded operator diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetMcsRuntimeSnapshot {
    pub registry: FaultRegistrySnapshot,
    pub registry_sealed: bool,
    pub fault_receiver_active: bool,
    pub root_control_active: bool,
    pub fatal: bool,
    pub recovered_timeout_mask: u64,
    pub pending_fault: bool,
    pub fault_endpoint_present: bool,
    pub root_fault_cnode_present: bool,
    pub driver_supervisor_cnode_present: bool,
}

#[must_use]
pub fn target_mcs_runtime_snapshot() -> Option<TargetMcsRuntimeSnapshot> {
    let registry = TARGET_FAULT_REGISTRY.try_lock()?.snapshot();
    Some(TargetMcsRuntimeSnapshot {
        registry,
        registry_sealed: TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire),
        fault_receiver_active: TARGET_FAULT_RECEIVER_ACTIVE.load(Ordering::Acquire),
        root_control_active: TARGET_ROOT_CONTROL_TEMPORAL_ACTIVE.load(Ordering::Acquire),
        fatal: TARGET_FATAL.load(Ordering::Acquire),
        recovered_timeout_mask: TARGET_RECOVERED_TIMEOUTS.load(Ordering::Acquire),
        pending_fault: TARGET_PENDING_FAULT_VALID.load(Ordering::Acquire),
        fault_endpoint_present: TARGET_FAULT_ENDPOINT.load(Ordering::Acquire) != 0,
        root_fault_cnode_present: TARGET_ROOT_FAULT_CNODE.load(Ordering::Acquire) != 0,
        driver_supervisor_cnode_present: TARGET_DRIVER_SUPERVISOR_CNODE.load(Ordering::Acquire)
            != 0,
    })
}

const SERVICE_RECOVERY_UNREGISTERED: usize = 0;
const SERVICE_RECOVERY_READY: usize = 1;
const SERVICE_RECOVERY_REPLIED: usize = 2;

/// Exact outcome of one root-to-service Call after the caller resumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetServiceCallCompletion {
    /// The service produced the ordinary protocol reply.
    Normal,
    /// Root-fault released the blocked caller after containing the service.
    Recovered {
        request_sequence: u64,
        fault_sequence: u64,
        fault_class: FaultClass,
    },
}

/// Return the exact standard and timeout badge pair for one generated task.
#[must_use]
pub fn temporal_fault_badges(task_id: &str) -> Option<(u64, u64)> {
    let task = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == task_id)?;
    Some((generated_standard_fault_badge(task_id)?, task.timeout_badge))
}

/// Return the root-owned shared fault endpoint origin after construction.
#[must_use]
pub fn target_fault_endpoint_origin() -> Option<seL4_CPtr> {
    let endpoint = TARGET_FAULT_ENDPOINT.load(Ordering::Acquire);
    (endpoint != 0).then_some(endpoint as seL4_CPtr)
}

fn target_service_mailbox(task_id: &str) -> Result<usize, CriticalTcbConstructionError> {
    let task_index = generated::temporal_tasks()
        .iter()
        .position(|task| task.id == task_id)
        .and_then(|index| u16::try_from(index).ok())
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    service_fault_mailbox_index(task_index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)
}

/// Install one passive service's retained Reply cap into the restricted
/// root-fault CSpace before the exact fault registry is sealed.
///
/// Active services are deliberately ineligible: only the generated NineDoor
/// `ReturnError` donation chain may use this cap to release a blocked donor.
pub fn register_target_service_recovery_reply(
    task_id: &str,
    reply_cap: seL4_CPtr,
) -> Result<(), CriticalTcbConstructionError> {
    if TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RegistrySealed);
    }
    if reply_cap == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::InvalidCapabilityRights);
    }
    let contract = passive_service_recovery_contract(task_id)
        .map_err(|_| CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let mailbox = contract.mailbox;
    if TARGET_SERVICE_RECOVERY_STATES[mailbox].load(Ordering::Acquire)
        != SERVICE_RECOVERY_UNREGISTERED
        || TARGET_SERVICE_RECOVERY_SLOTS[mailbox].load(Ordering::Acquire) != 0
        || TARGET_SERVICE_CALL_SEQUENCES[mailbox].load(Ordering::Acquire) != 0
        || TARGET_SERVICE_RECOVERY_REQUEST_SEQUENCES[mailbox].load(Ordering::Acquire) != 0
        || TARGET_SERVICE_RECOVERY_FAULT_SEQUENCES[mailbox].load(Ordering::Acquire) != 0
        || TARGET_SERVICE_RECOVERY_FAULT_CLASSES[mailbox].load(Ordering::Acquire) != 0
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault_cnode = TARGET_ROOT_FAULT_CNODE.load(Ordering::Acquire) as seL4_CPtr;
    if root_fault_cnode == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault = critical_resource(ROOT_FAULT_ID)?;
    let destination = seL4_CPtr::from(contract.root_fault_reply_slot);
    let error = sel4::cnode_copy_depth(
        root_fault_cnode,
        destination,
        root_fault.cnode_radix_bits,
        sel4_sys::seL4_CapInitThreadCNode,
        reply_cap,
        sel4::word_bits() as u8,
        sel4_sys::seL4_CapRights_All,
    );
    if error != sel4_sys::seL4_NoError {
        return Err(sel4_error("critical.service-recovery-reply-copy", error));
    }
    TARGET_SERVICE_RECOVERY_SLOTS[mailbox].store(destination as usize, Ordering::Release);
    TARGET_SERVICE_RECOVERY_STATES[mailbox].store(SERVICE_RECOVERY_READY, Ordering::Release);
    Ok(())
}

/// Arm the exact one-in-flight service request before root donates its SC.
pub fn arm_target_service_call(
    task_id: &str,
    sequence: u64,
) -> Result<(), CriticalTcbConstructionError> {
    let mailbox = target_service_mailbox(task_id)?;
    if sequence == 0
        || TARGET_SERVICE_RECOVERY_STATES[mailbox].load(Ordering::Acquire) != SERVICE_RECOVERY_READY
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    TARGET_SERVICE_CALL_SEQUENCES[mailbox]
        .compare_exchange(0, sequence, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
    Ok(())
}

/// Clear one completed service request after a normal reply or validate that
/// root-fault already issued its one typed recovery reply.
pub fn finish_target_service_call(
    task_id: &str,
    sequence: u64,
) -> Result<TargetServiceCallCompletion, CriticalTcbConstructionError> {
    let mailbox = target_service_mailbox(task_id)?;
    match TARGET_SERVICE_RECOVERY_STATES[mailbox].load(Ordering::Acquire) {
        SERVICE_RECOVERY_READY => TARGET_SERVICE_CALL_SEQUENCES[mailbox]
            .compare_exchange(sequence, 0, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| TargetServiceCallCompletion::Normal)
            .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady),
        SERVICE_RECOVERY_REPLIED
            if TARGET_SERVICE_CALL_SEQUENCES[mailbox].load(Ordering::Acquire) == 0 =>
        {
            let request_sequence =
                TARGET_SERVICE_RECOVERY_REQUEST_SEQUENCES[mailbox].load(Ordering::Acquire);
            let fault_sequence =
                TARGET_SERVICE_RECOVERY_FAULT_SEQUENCES[mailbox].load(Ordering::Acquire);
            let fault_class =
                match TARGET_SERVICE_RECOVERY_FAULT_CLASSES[mailbox].load(Ordering::Acquire) {
                    1 => FaultClass::Standard,
                    2 => FaultClass::Timeout,
                    _ => return Err(CriticalTcbConstructionError::RuntimeNotReady),
                };
            if request_sequence != sequence || fault_sequence == 0 {
                return Err(CriticalTcbConstructionError::RuntimeNotReady);
            }
            Ok(TargetServiceCallCompletion::Recovered {
                request_sequence,
                fault_sequence,
                fault_class,
            })
        }
        _ => Err(CriticalTcbConstructionError::RuntimeNotReady),
    }
}

/// Remove a contained service's recovery cap and prevent old-generation Reply
/// authority from surviving anchor revoke or reconstruction.
pub fn revoke_target_service_recovery_reply(
    task_id: &str,
) -> Result<(), CriticalTcbConstructionError> {
    revoke_target_service_recovery_reply_with(task_id, sel4::cnode_delete)
}

/// Remove the retained passive-service Reply cap with one quiet kernel action.
pub(crate) fn revoke_target_service_recovery_reply_bounded(
    task_id: &str,
) -> Result<(), CriticalTcbConstructionError> {
    revoke_target_service_recovery_reply_with(task_id, sel4::cnode_delete_bounded)
}

fn revoke_target_service_recovery_reply_with(
    task_id: &str,
    delete: fn(seL4_CPtr, seL4_CPtr, u8) -> sel4_sys::seL4_Error,
) -> Result<(), CriticalTcbConstructionError> {
    let mailbox = target_service_mailbox(task_id)?;
    if TARGET_SERVICE_CALL_SEQUENCES[mailbox].load(Ordering::Acquire) != 0 {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let slot = TARGET_SERVICE_RECOVERY_SLOTS[mailbox].load(Ordering::Acquire) as seL4_CPtr;
    let root_fault_cnode = TARGET_ROOT_FAULT_CNODE.load(Ordering::Acquire) as seL4_CPtr;
    if slot == sel4_sys::seL4_CapNull || root_fault_cnode == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault = critical_resource(ROOT_FAULT_ID)?;
    let error = delete(root_fault_cnode, slot, root_fault.cnode_radix_bits);
    if error != sel4_sys::seL4_NoError {
        return Err(sel4_error("critical.service-recovery-reply-delete", error));
    }
    TARGET_SERVICE_RECOVERY_SLOTS[mailbox].store(0, Ordering::Release);
    TARGET_SERVICE_RECOVERY_REQUEST_SEQUENCES[mailbox].store(0, Ordering::Release);
    TARGET_SERVICE_RECOVERY_FAULT_SEQUENCES[mailbox].store(0, Ordering::Release);
    TARGET_SERVICE_RECOVERY_FAULT_CLASSES[mailbox].store(0, Ordering::Release);
    TARGET_SERVICE_RECOVERY_STATES[mailbox].store(SERVICE_RECOVERY_UNREGISTERED, Ordering::Release);
    Ok(())
}

/// Exact outcome after one executor resumes from a passive Worker Call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetWorkerCallCompletion {
    /// The Worker returned the ordinary ABI reply.
    Normal,
    /// Root-fault contained the Worker and released this exact donor once.
    Recovered {
        request_sequence: u64,
        fault_sequence: u64,
        fault_class: FaultClass,
    },
}

fn worker_recovery_reply_slot(
    worker_index: usize,
) -> Result<seL4_CPtr, CriticalTcbConstructionError> {
    if worker_index >= MAX_EXECUTABLE_WORKER_SLOTS {
        return Err(CriticalTcbConstructionError::MissingGeneratedRecord);
    }
    let base = seL4_CPtr::from(generated::ninedoor_service_config().root_fault_recovery_reply_slot)
        .checked_add(1)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let slot = base
        .checked_add(
            seL4_CPtr::try_from(worker_index)
                .map_err(|_| CriticalTcbConstructionError::MissingGeneratedRecord)?,
        )
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let tcb_base = seL4_CPtr::from(
        generated::worker_resource_admission_config()
            .fault_registry
            .root_fault_tcb_control_slot_base,
    );
    if slot >= tcb_base {
        return Err(CriticalTcbConstructionError::MissingGeneratedRecord);
    }
    Ok(slot)
}

/// Copy one Worker's single-owner Reply object into its exact root-fault slot.
pub fn register_target_worker_recovery_reply(
    worker_index: usize,
    reply_cap: seL4_CPtr,
) -> Result<(), CriticalTcbConstructionError> {
    let state = TARGET_WORKER_RECOVERY_STATES
        .get(worker_index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let slot_state = TARGET_WORKER_RECOVERY_SLOTS
        .get(worker_index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    if reply_cap == sel4_sys::seL4_CapNull
        || state.load(Ordering::Acquire) != SERVICE_RECOVERY_UNREGISTERED
        || slot_state.load(Ordering::Acquire) != 0
        || TARGET_WORKER_CALL_SEQUENCES[worker_index].load(Ordering::Acquire) != 0
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault_cnode = TARGET_ROOT_FAULT_CNODE.load(Ordering::Acquire) as seL4_CPtr;
    if root_fault_cnode == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault = critical_resource(ROOT_FAULT_ID)?;
    let destination = worker_recovery_reply_slot(worker_index)?;
    let error = sel4::cnode_copy_depth(
        root_fault_cnode,
        destination,
        root_fault.cnode_radix_bits,
        sel4_sys::seL4_CapInitThreadCNode,
        reply_cap,
        sel4::word_bits() as u8,
        sel4_sys::seL4_CapRights_All,
    );
    if error != sel4_sys::seL4_NoError {
        return Err(sel4_error("critical.worker-recovery-reply-copy", error));
    }
    slot_state.store(destination as usize, Ordering::Release);
    state.store(SERVICE_RECOVERY_READY, Ordering::Release);
    Ok(())
}

/// Arm one exact executor Call immediately before its MCS donation.
pub fn arm_target_worker_call(
    worker_index: usize,
    sequence: u64,
    identity: worker_task_abi::WorkerIdentity,
) -> Result<(), CriticalTcbConstructionError> {
    if sequence == 0
        || identity.validate().is_err()
        || TARGET_WORKER_RECOVERY_STATES
            .get(worker_index)
            .is_none_or(|state| state.load(Ordering::Acquire) != SERVICE_RECOVERY_READY)
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    TARGET_WORKER_CALL_ROLE_SLOTS[worker_index].store(identity.slot as usize, Ordering::Relaxed);
    TARGET_WORKER_CALL_SUPERVISOR_GENERATIONS[worker_index]
        .store(identity.supervisor_generation, Ordering::Relaxed);
    TARGET_WORKER_CALL_CAP_GENERATIONS[worker_index]
        .store(identity.cap_generation, Ordering::Relaxed);
    TARGET_WORKER_CALL_SEQUENCES[worker_index]
        .compare_exchange(0, sequence, Ordering::Release, Ordering::Acquire)
        .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
    Ok(())
}

/// Finish one executor Call, distinguishing an ordinary reply from fault recovery.
pub fn finish_target_worker_call(
    worker_index: usize,
    sequence: u64,
) -> Result<TargetWorkerCallCompletion, CriticalTcbConstructionError> {
    let state = TARGET_WORKER_RECOVERY_STATES
        .get(worker_index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?
        .load(Ordering::Acquire);
    match state {
        SERVICE_RECOVERY_READY => TARGET_WORKER_CALL_SEQUENCES[worker_index]
            .compare_exchange(sequence, 0, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| TargetWorkerCallCompletion::Normal)
            .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady),
        SERVICE_RECOVERY_REPLIED
            if TARGET_WORKER_CALL_SEQUENCES[worker_index].load(Ordering::Acquire) == 0 =>
        {
            let request_sequence =
                TARGET_WORKER_RECOVERY_REQUEST_SEQUENCES[worker_index].load(Ordering::Acquire);
            let fault_sequence =
                TARGET_WORKER_RECOVERY_FAULT_SEQUENCES[worker_index].load(Ordering::Acquire);
            let fault_class =
                match TARGET_WORKER_RECOVERY_FAULT_CLASSES[worker_index].load(Ordering::Acquire) {
                    1 => FaultClass::Standard,
                    2 => FaultClass::Timeout,
                    _ => return Err(CriticalTcbConstructionError::RuntimeNotReady),
                };
            if request_sequence != sequence || fault_sequence == 0 {
                return Err(CriticalTcbConstructionError::RuntimeNotReady);
            }
            Ok(TargetWorkerCallCompletion::Recovered {
                request_sequence,
                fault_sequence,
                fault_class,
            })
        }
        _ => Err(CriticalTcbConstructionError::RuntimeNotReady),
    }
}

/// Remove one contained Worker's root-fault Reply view before anchor reuse.
pub fn revoke_target_worker_recovery_reply(
    worker_index: usize,
) -> Result<(), CriticalTcbConstructionError> {
    if worker_index >= MAX_EXECUTABLE_WORKER_SLOTS
        || TARGET_WORKER_CALL_SEQUENCES[worker_index].load(Ordering::Acquire) != 0
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let slot = TARGET_WORKER_RECOVERY_SLOTS[worker_index].load(Ordering::Acquire) as seL4_CPtr;
    let root_fault_cnode = TARGET_ROOT_FAULT_CNODE.load(Ordering::Acquire) as seL4_CPtr;
    if slot == sel4_sys::seL4_CapNull || root_fault_cnode == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault = critical_resource(ROOT_FAULT_ID)?;
    let error = sel4::cnode_delete(root_fault_cnode, slot, root_fault.cnode_radix_bits);
    if error != sel4_sys::seL4_NoError {
        return Err(sel4_error("critical.worker-recovery-reply-delete", error));
    }
    TARGET_WORKER_RECOVERY_SLOTS[worker_index].store(0, Ordering::Release);
    TARGET_WORKER_CALL_ROLE_SLOTS[worker_index].store(0, Ordering::Relaxed);
    TARGET_WORKER_CALL_SUPERVISOR_GENERATIONS[worker_index].store(0, Ordering::Relaxed);
    TARGET_WORKER_CALL_CAP_GENERATIONS[worker_index].store(0, Ordering::Relaxed);
    TARGET_WORKER_RECOVERY_REQUEST_SEQUENCES[worker_index].store(0, Ordering::Relaxed);
    TARGET_WORKER_RECOVERY_FAULT_SEQUENCES[worker_index].store(0, Ordering::Relaxed);
    TARGET_WORKER_RECOVERY_FAULT_CLASSES[worker_index].store(0, Ordering::Relaxed);
    TARGET_WORKER_RECOVERY_STATES[worker_index]
        .store(SERVICE_RECOVERY_UNREGISTERED, Ordering::Release);
    Ok(())
}

/// Report whether the independently scheduled root-fault receiver is active.
///
/// Deferred service and Worker children use this release/acquire boundary to
/// avoid becoming runnable before the sole MCS fault receiver can accept their
/// first standard or timeout fault.
#[must_use]
pub fn target_fault_receiver_active() -> bool {
    TARGET_FAULT_RECEIVER_ACTIVE.load(Ordering::Acquire)
}

fn root_fault_tcb_control_slot(task_index: u16) -> Result<seL4_CPtr, CriticalTcbConstructionError> {
    let resource = critical_resource(ROOT_FAULT_ID)?;
    let slot_base = seL4_CPtr::from(
        generated::worker_resource_admission_config()
            .fault_registry
            .root_fault_tcb_control_slot_base,
    );
    let slot = slot_base
        .checked_add(seL4_CPtr::from(task_index))
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let capacity = 1u64
        .checked_shl(u32::from(resource.cnode_radix_bits))
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    if slot >= capacity {
        return Err(CriticalTcbConstructionError::MissingGeneratedRecord);
    }
    Ok(slot)
}

fn install_root_fault_tcb_control_cap(
    task_index: u16,
    root_tcb_cap: seL4_CPtr,
) -> Result<seL4_CPtr, CriticalTcbConstructionError> {
    let index = usize::from(task_index);
    let target = TARGET_ROOT_FAULT_TCB_CAP_SLOTS
        .get(index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    if root_tcb_cap == sel4_sys::seL4_CapNull || target.load(Ordering::Acquire) != 0 {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault_cnode = TARGET_ROOT_FAULT_CNODE.load(Ordering::Acquire) as seL4_CPtr;
    if root_fault_cnode == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let resource = critical_resource(ROOT_FAULT_ID)?;
    let slot = root_fault_tcb_control_slot(task_index)?;
    let error = sel4::cnode_copy_depth(
        root_fault_cnode,
        slot,
        resource.cnode_radix_bits,
        sel4_sys::seL4_CapInitThreadCNode,
        root_tcb_cap,
        sel4::word_bits() as u8,
        sel4_sys::seL4_CapRights_All,
    );
    if error != sel4_sys::seL4_NoError {
        return Err(sel4_error("critical.root-fault-tcb-control-copy", error));
    }
    target.store(slot as usize, Ordering::Release);
    Ok(slot)
}

fn replace_root_fault_tcb_control_cap(
    task_index: u16,
    root_tcb_cap: seL4_CPtr,
) -> Result<seL4_CPtr, CriticalTcbConstructionError> {
    let index = usize::from(task_index);
    let target = TARGET_ROOT_FAULT_TCB_CAP_SLOTS
        .get(index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let slot = root_fault_tcb_control_slot(task_index)?;
    if root_tcb_cap == sel4_sys::seL4_CapNull || target.load(Ordering::Acquire) != slot as usize {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_fault_cnode = TARGET_ROOT_FAULT_CNODE.load(Ordering::Acquire) as seL4_CPtr;
    if root_fault_cnode == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let resource = critical_resource(ROOT_FAULT_ID)?;
    let delete_error = sel4::cnode_delete(root_fault_cnode, slot, resource.cnode_radix_bits);
    if delete_error != sel4_sys::seL4_NoError {
        return Err(sel4_error(
            "critical.root-fault-tcb-control-delete",
            delete_error,
        ));
    }
    let copy_error = sel4::cnode_copy_depth(
        root_fault_cnode,
        slot,
        resource.cnode_radix_bits,
        sel4_sys::seL4_CapInitThreadCNode,
        root_tcb_cap,
        sel4::word_bits() as u8,
        sel4_sys::seL4_CapRights_All,
    );
    if copy_error != sel4_sys::seL4_NoError {
        target.store(0, Ordering::Release);
        return Err(sel4_error(
            "critical.root-fault-tcb-control-replace",
            copy_error,
        ));
    }
    Ok(slot)
}

fn root_fault_tcb_control_cap(task_index: u16) -> Result<seL4_CPtr, CriticalTcbConstructionError> {
    let expected = root_fault_tcb_control_slot(task_index)?;
    let cap = TARGET_ROOT_FAULT_TCB_CAP_SLOTS
        .get(usize::from(task_index))
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?
        .load(Ordering::Acquire) as seL4_CPtr;
    if cap != expected {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    Ok(cap)
}

fn driver_supervisor_runtime_cap_slot(
    runtime_slot: u16,
    offset: seL4_CPtr,
) -> Result<seL4_CPtr, CriticalTcbConstructionError> {
    let configured = generated::worker_resource_admission_config()
        .handoff
        .driver_fault_records;
    if runtime_slot >= configured || offset >= DRIVER_SUPERVISOR_RUNTIME_CAP_STRIDE {
        return Err(CriticalTcbConstructionError::MissingGeneratedRecord);
    }
    DRIVER_SUPERVISOR_RUNTIME_CAP_SLOT_BASE
        .checked_add(
            seL4_CPtr::from(runtime_slot)
                .checked_mul(DRIVER_SUPERVISOR_RUNTIME_CAP_STRIDE)
                .ok_or(CriticalTcbConstructionError::InvalidCapabilityRights)?,
        )
        .and_then(|slot| slot.checked_add(offset))
        .ok_or(CriticalTcbConstructionError::InvalidCapabilityRights)
}

/// Transfer one linked runtime's retained origins into driver-supervisor.
///
/// The TCB is copied because root-fault and root-control retain their own
/// separately declared views. Every other supplied cap is moved, preserving
/// its MDB ancestry so a revoke issued against the child-local origin removes
/// the old generation's derived root/driver caps without exposing root CSpace.
pub fn install_driver_supervisor_runtime_caps(
    runtime_slot: u16,
    caps: DriverSupervisorRuntimeRootCaps,
) -> Result<DriverSupervisorRuntimeLocalCaps, CriticalTcbConstructionError> {
    if [
        caps.tcb,
        caps.command_endpoint_origin,
        caps.command_reply,
        caps.completion_notification_origin,
        caps.sched_context,
        caps.standard_fault_endpoint,
        caps.timeout_fault_endpoint,
    ]
    .contains(&sel4_sys::seL4_CapNull)
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let supervisor_cnode = TARGET_DRIVER_SUPERVISOR_CNODE.load(Ordering::Acquire) as seL4_CPtr;
    if supervisor_cnode == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let resource = critical_resource(DRIVER_SUPERVISOR_ID)?;
    let child_depth = resource.cnode_radix_bits;
    let child_slots = 1usize
        .checked_shl(u32::from(child_depth))
        .ok_or(CriticalTcbConstructionError::InvalidCapabilityRights)?;
    let tcb_slot =
        driver_supervisor_runtime_cap_slot(runtime_slot, DRIVER_SUPERVISOR_RUNTIME_TCB_OFFSET)?;
    let final_slot = driver_supervisor_runtime_cap_slot(
        runtime_slot,
        DRIVER_SUPERVISOR_RUNTIME_TIMEOUT_FAULT_OFFSET,
    )?;
    if final_slot as usize >= child_slots {
        return Err(CriticalTcbConstructionError::InvalidCapabilityRights);
    }
    let root_cnode = sel4_sys::seL4_CapInitThreadCNode;
    let root_depth = sel4::word_bits() as u8;
    let copy_error = sel4::cnode_copy_depth(
        supervisor_cnode,
        tcb_slot,
        child_depth,
        root_cnode,
        caps.tcb,
        root_depth,
        sel4_sys::seL4_CapRights_All,
    );
    if copy_error != sel4_sys::seL4_NoError {
        return Err(sel4_error(
            "critical.driver-supervisor-tcb-control-copy",
            copy_error,
        ));
    }

    let move_rows = [
        (
            caps.command_endpoint_origin,
            DRIVER_SUPERVISOR_RUNTIME_COMMAND_ORIGIN_OFFSET,
        ),
        (
            caps.command_reply,
            DRIVER_SUPERVISOR_RUNTIME_COMMAND_REPLY_OFFSET,
        ),
        (
            caps.completion_notification_origin,
            DRIVER_SUPERVISOR_RUNTIME_COMPLETION_ORIGIN_OFFSET,
        ),
        (caps.sched_context, DRIVER_SUPERVISOR_RUNTIME_SC_OFFSET),
        (
            caps.standard_fault_endpoint,
            DRIVER_SUPERVISOR_RUNTIME_STANDARD_FAULT_OFFSET,
        ),
        (
            caps.timeout_fault_endpoint,
            DRIVER_SUPERVISOR_RUNTIME_TIMEOUT_FAULT_OFFSET,
        ),
    ];
    let mut moved = 0usize;
    for (root_slot, offset) in move_rows {
        let child_slot = driver_supervisor_runtime_cap_slot(runtime_slot, offset)?;
        let error = sel4::cnode_move_depth(
            supervisor_cnode,
            child_slot,
            child_depth,
            root_cnode,
            root_slot,
            root_depth,
        );
        if error != sel4_sys::seL4_NoError {
            for rollback in (0..moved).rev() {
                let (prior_root_slot, prior_offset) = move_rows[rollback];
                let prior_child_slot =
                    driver_supervisor_runtime_cap_slot(runtime_slot, prior_offset)?;
                let _ = sel4::cnode_move_depth(
                    root_cnode,
                    prior_root_slot,
                    root_depth,
                    supervisor_cnode,
                    prior_child_slot,
                    child_depth,
                );
            }
            let _ = sel4::cnode_delete(supervisor_cnode, tcb_slot, child_depth);
            return Err(sel4_error(
                "critical.driver-supervisor-authority-move",
                error,
            ));
        }
        moved = moved.saturating_add(1);
    }

    Ok(DriverSupervisorRuntimeLocalCaps {
        tcb: tcb_slot,
        command_endpoint_origin: driver_supervisor_runtime_cap_slot(
            runtime_slot,
            DRIVER_SUPERVISOR_RUNTIME_COMMAND_ORIGIN_OFFSET,
        )?,
        command_reply: driver_supervisor_runtime_cap_slot(
            runtime_slot,
            DRIVER_SUPERVISOR_RUNTIME_COMMAND_REPLY_OFFSET,
        )?,
        completion_notification_origin: driver_supervisor_runtime_cap_slot(
            runtime_slot,
            DRIVER_SUPERVISOR_RUNTIME_COMPLETION_ORIGIN_OFFSET,
        )?,
        sched_context: driver_supervisor_runtime_cap_slot(
            runtime_slot,
            DRIVER_SUPERVISOR_RUNTIME_SC_OFFSET,
        )?,
        standard_fault_endpoint: driver_supervisor_runtime_cap_slot(
            runtime_slot,
            DRIVER_SUPERVISOR_RUNTIME_STANDARD_FAULT_OFFSET,
        )?,
        timeout_fault_endpoint: driver_supervisor_runtime_cap_slot(
            runtime_slot,
            DRIVER_SUPERVISOR_RUNTIME_TIMEOUT_FAULT_OFFSET,
        )?,
    })
}

/// Revoke descendants of one exact driver-supervisor-local origin cap.
pub fn driver_supervisor_revoke_local_cap(cap: seL4_CPtr) -> seL4_Error {
    let depth = critical_resource(DRIVER_SUPERVISOR_ID)
        .map(|resource| resource.cnode_radix_bits)
        .unwrap_or(0);
    if depth == 0 {
        return sel4_sys::seL4_FailedLookup;
    }
    sel4::cnode_revoke(CHILD_SELF_CNODE_SLOT, cap, depth)
}

/// Delete one exact driver-supervisor-local retained cap.
pub fn driver_supervisor_delete_local_cap(cap: seL4_CPtr) -> seL4_Error {
    let depth = critical_resource(DRIVER_SUPERVISOR_ID)
        .map(|resource| resource.cnode_radix_bits)
        .unwrap_or(0);
    if depth == 0 {
        return sel4_sys::seL4_FailedLookup;
    }
    sel4::cnode_delete(CHILD_SELF_CNODE_SLOT, cap, depth)
}

/// Register one constructed live TCB in the exact target fault registry.
///
/// Service, Worker, and driver constructors call this before critical-domain
/// activation. Badges are always derived from generated truth; callers supply
/// only the retained TCB cap and immutable generation identity.
pub fn register_target_fault_source(
    task_id: &str,
    tcb_cap: seL4_CPtr,
    identity: GenerationIdentity,
) -> Result<(), CriticalTcbConstructionError> {
    if TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RegistrySealed);
    }
    let tasks = generated::temporal_tasks();
    let task_index = tasks
        .iter()
        .position(|task| task.id == task_id)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let task = &tasks[task_index];
    let (standard_badge, timeout_badge) = temporal_fault_badges(task_id)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let task_index = u16::try_from(task_index)
        .map_err(|_| CriticalTcbConstructionError::MissingGeneratedRecord)?;
    TARGET_FAULT_REGISTRY.lock().register(FaultRegistration {
        task_index,
        identity,
        standard_badge,
        timeout_badge,
        tcb_cap: tcb_cap as usize,
        terminal: task.timeout_policy != TimeoutPolicy::ReplenishOnce,
    })?;
    install_root_fault_tcb_control_cap(task_index, tcb_cap)?;
    let mut line = heapless::String::<256>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[diag fault-registry/v1] register index={} id={} kind={:?} tcb=0x{:04x} standard_badge=0x{:08x} timeout_badge=0x{:08x}",
            task_index,
            task_id,
            task.kind,
            tcb_cap,
            standard_badge,
            timeout_badge,
        ),
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
    Ok(())
}

/// Replace one sealed source after complete containment and prior-cap revoke.
///
/// The generated task index and badge pair cannot change. The caller supplies
/// the exact prior identity, and the registry accepts only a non-null TCB with
/// strictly newer supervisor and capability generations.
pub fn replace_target_fault_source(
    task_id: &str,
    prior_identity: GenerationIdentity,
    new_tcb_cap: seL4_CPtr,
    new_identity: GenerationIdentity,
) -> Result<(), CriticalTcbConstructionError> {
    if !TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RegistryNotSealed);
    }
    let tasks = generated::temporal_tasks();
    let task_index = tasks
        .iter()
        .position(|task| task.id == task_id)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let task = &tasks[task_index];
    let (standard_badge, timeout_badge) = temporal_fault_badges(task_id)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let task_index = u16::try_from(task_index)
        .map_err(|_| CriticalTcbConstructionError::MissingGeneratedRecord)?;
    TARGET_FAULT_REGISTRY.lock().replace(
        prior_identity,
        FaultRegistration {
            task_index,
            identity: new_identity,
            standard_badge,
            timeout_badge,
            tcb_cap: new_tcb_cap as usize,
            terminal: task.timeout_policy != TimeoutPolicy::ReplenishOnce,
        },
    )?;
    replace_root_fault_tcb_control_cap(task_index, new_tcb_cap)?;
    Ok(())
}

/// Seal the target registry after all critical/service/Worker/driver TCBs exist.
pub fn seal_target_fault_registry() -> Result<(), CriticalTcbConstructionError> {
    if TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RegistrySealed);
    }
    let tasks = generated::temporal_tasks();
    let mut registry = TARGET_FAULT_REGISTRY.lock();
    let mut summary = heapless::String::<128>::new();
    let _ = core::fmt::write(
        &mut summary,
        format_args!(
            "[diag fault-registry/v1] seal begin registered={} expected={}",
            registry.len(),
            tasks.len(),
        ),
    );
    crate::bootstrap::log::force_uart_line(summary.as_str());
    if registry.len() != tasks.len() {
        for (index, task) in tasks.iter().enumerate() {
            let Ok(task_index) = u16::try_from(index) else {
                break;
            };
            if registry.contains_task_index(task_index) {
                continue;
            }
            let mut missing = heapless::String::<160>::new();
            let _ = core::fmt::write(
                &mut missing,
                format_args!(
                    "[diag fault-registry/v1] missing index={} id={} kind={:?}",
                    task_index, task.id, task.kind,
                ),
            );
            crate::bootstrap::log::force_uart_line(missing.as_str());
        }
    }
    registry.seal()?;
    drop(registry);
    TARGET_FAULT_REGISTRY_SEALED.store(true, Ordering::Release);
    crate::bootstrap::log::force_uart_line("[diag fault-registry/v1] seal result=ok");
    Ok(())
}

/// Nonblockingly publish root-control work and emit one coalesced wake.
pub fn publish_worker_control(
    record: WorkerControlRecord,
    worker_signal_cap: seL4_CPtr,
) -> PublishResult {
    if worker_signal_cap == sel4_sys::seL4_CapNull || TARGET_FATAL.load(Ordering::Acquire) {
        return PublishResult::Refused;
    }
    let result = TARGET_WORKER_CONTROL.publish(record);
    if result == PublishResult::Published {
        sel4::signal_unchecked(worker_signal_cap);
    }
    result
}

/// Consume one record only after the restricted Worker-supervisor has
/// validated it in place. Root-control remains the sole lifecycle authority.
pub(crate) fn take_validated_worker_control(
) -> Result<Option<WorkerControlRecord>, WorkerControlQueueError> {
    TARGET_WORKER_CONTROL.drain_validated()
}

/// Nonblockingly take one suspended service fault for its root-control owner.
///
/// The root event loop calls this with a generated service ID and performs the
/// service-specific caller failure and revoke sequence outside the restricted
/// root-fault TCB. Contention is explicit so the next bounded turn can retry
/// without losing the durable record.
pub fn take_target_service_fault(
    task_id: &str,
) -> Result<Option<FaultHandoffRecord>, CriticalTcbConstructionError> {
    let task_index = generated::temporal_tasks()
        .iter()
        .position(|task| task.id == task_id)
        .and_then(|index| u16::try_from(index).ok())
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
        return Err(CriticalTcbConstructionError::FaultHandoff(
            FaultHandoffError::Contended,
        ));
    };
    handoff
        .drain_service(task_index)
        .map_err(CriticalTcbConstructionError::FaultHandoff)
}

/// Resume all four restricted duties only after exact registry construction.
///
/// The init/root-control TCB retains its bootstrap scheduling context until
/// userland has finished construction and is about to enter the steady event
/// loop. Applying its generated 2.75 ms budget here would charge remaining
/// bootstrap work to a steady-state WCET contract and can trigger a legitimate
/// timeout before the runtime boundary exists.
pub fn activate_critical_tcb_runtime(
    runtime: &CriticalTcbRuntime,
) -> Result<(), CriticalTcbConstructionError> {
    if !TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RegistryNotSealed);
    }
    if TARGET_FATAL.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    TARGET_FAULT_RECEIVER_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
    #[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
    cohesix_critical_runtime_qemu_evidence_arm();
    for index in 1..CRITICAL_TCB_COUNT {
        let tcb = runtime.handles[index].tcb_cap as seL4_CPtr;
        if let Err(error) = sel4::resume_tcb(tcb) {
            let mut rollback_complete = true;
            for prior in 1..index {
                rollback_complete &=
                    sel4::suspend_tcb(runtime.handles[prior].tcb_cap as seL4_CPtr).is_ok();
            }
            if rollback_complete {
                TARGET_FAULT_RECEIVER_ACTIVE.store(false, Ordering::Release);
            } else {
                TARGET_FATAL.store(true, Ordering::Release);
            }
            return Err(sel4_error("critical.child-resume", error));
        }
    }
    Ok(())
}

/// Stable external-QEMU observation point before any restricted duty resumes.
///
/// A halted SMP boot reaches this point only after seL4 has initialized every
/// secondary core. The collector can therefore re-arm accelerator hardware
/// breakpoints here without changing target scheduling or runtime authority.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_critical_runtime_qemu_evidence_arm() {
    core::hint::black_box(0x26e0_0000_u32);
}

/// Apply the generated root-control SC parameters at the steady event-loop boundary.
pub fn activate_root_control_temporal_runtime(
    runtime: &CriticalTcbRuntime,
) -> Result<(), CriticalTcbConstructionError> {
    if !TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire)
        || !TARGET_FAULT_RECEIVER_ACTIVE.load(Ordering::Acquire)
        || TARGET_FATAL.load(Ordering::Acquire)
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let root_control = runtime.handles[0];
    let root_task = temporal_task(ROOT_CONTROL_ID)?;
    TARGET_ROOT_CONTROL_TEMPORAL_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
    if let Err(error) = configure_active_sc_with_sched_control(
        root_task,
        root_control.tcb_cap as seL4_CPtr,
        root_control.sched_context_cap as seL4_CPtr,
        root_control.sched_control_cap as seL4_CPtr,
        root_control.fault_endpoint_cap as seL4_CPtr,
        root_control.timeout_endpoint_cap as seL4_CPtr,
    ) {
        TARGET_ROOT_CONTROL_TEMPORAL_ACTIVE.store(false, Ordering::Release);
        return Err(error);
    }
    Ok(())
}

/// Construct the exact MCS critical-domain topology in a suspended state.
///
/// No restricted duty becomes runnable until every admitted temporal TCB has
/// registered and [`activate_critical_tcb_runtime`] seals the complete graph.
pub fn construct_critical_tcb_runtime(
    env: &mut KernelEnv<'_>,
    fault_endpoint: seL4_CPtr,
    entrypoints: CriticalTcbEntrypoints,
) -> Result<CriticalTcbRuntime, CriticalTcbConstructionError> {
    validate_critical_temporal_graph()?;
    entrypoints.validate()?;
    if fault_endpoint == sel4_sys::seL4_CapNull {
        return Err(sel4_error(
            "critical.fault-endpoint",
            sel4_sys::seL4_InvalidCapability,
        ));
    }
    validate_generated_rights()?;

    let admission = generated::worker_resource_admission_config();
    for resource in admission.critical_tcbs {
        // These permanent critical domains retain a CNode cap in the
        // manifest-named slot. Unlike executable Worker generations, their
        // objects are not grouped beneath a reclaimable child untyped.
        let retention_slot = seL4_CPtr::try_from(resource.revoke_anchor_slot)
            .map_err(|_| sel4_error("critical.retention-convert", sel4_sys::seL4_RangeError))?;
        env.reserve_cspace_anchor_slot(retention_slot)
            .map_err(|error| sel4_error("critical.retention-reserve", error))?;
    }

    let emergency_endpoint = env
        .alloc_endpoint()
        .map_err(|error| sel4_error("critical.emergency-endpoint", error))?;
    if TARGET_FAULT_ENDPOINT
        .compare_exchange(
            0,
            fault_endpoint as usize,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let worker_wake = env
        .alloc_notification()
        .map_err(|error| sel4_error("critical.worker-wake", error))?;
    let driver_wake = env
        .alloc_notification()
        .map_err(|error| sel4_error("critical.driver-wake", error))?;
    let emergency_wake = env
        .alloc_notification()
        .map_err(|error| sel4_error("critical.emergency-wake", error))?;
    let root_control_wake = env
        .alloc_notification()
        .map_err(|error| sel4_error("critical.root-control-wake", error))?;
    let root_fault_wake = env
        .alloc_notification()
        .map_err(|error| sel4_error("critical.root-fault-wake", error))?;
    let worker_executor_gpu_wake = env
        .alloc_notification()
        .map_err(|error| sel4_error("critical.worker-executor-gpu-wake", error))?;
    let worker_executor_lora_wake = env
        .alloc_notification()
        .map_err(|error| sel4_error("critical.worker-executor-lora-wake", error))?;

    let signals = CriticalSignalCaps {
        worker_supervisor_origin: worker_wake,
        worker_supervisor: mint_root_badged_cap(
            env,
            worker_wake,
            write_only_rights(),
            admission.handoff.worker_wake_badge,
            "critical.worker-signal",
        )?,
        driver_supervisor: mint_root_badged_cap(
            env,
            driver_wake,
            write_only_rights(),
            admission.handoff.driver_wake_badge,
            "critical.driver-signal",
        )?,
        emergency: mint_root_badged_cap(
            env,
            emergency_wake,
            write_only_rights(),
            admission.handoff.emergency_wake_badge,
            "critical.emergency-signal",
        )?,
        root_fault_release: mint_root_badged_cap(
            env,
            root_fault_wake,
            write_only_rights(),
            admission.handoff.root_fault_release_badge,
            "critical.root-fault-release-signal",
        )?,
        worker_executor_gpu: mint_root_badged_cap(
            env,
            worker_executor_gpu_wake,
            write_only_rights(),
            1,
            "critical.worker-executor-gpu-signal",
        )?,
        worker_executor_lora: mint_root_badged_cap(
            env,
            worker_executor_lora_wake,
            write_only_rights(),
            1,
            "critical.worker-executor-lora-signal",
        )?,
        worker_executor_completion_gpu: mint_root_badged_cap(
            env,
            worker_wake,
            write_only_rights(),
            generated::worker_runtime_config().task_abi.gpu_wake_bit,
            "critical.worker-executor-gpu-completion",
        )?,
        worker_executor_completion_lora: mint_root_badged_cap(
            env,
            worker_wake,
            write_only_rights(),
            generated::worker_runtime_config()
                .task_abi
                .heartbeat_wake_bit
                | generated::worker_runtime_config().task_abi.lora_wake_bit,
            "critical.worker-executor-lora-completion",
        )?,
    };

    let mut inventory = CriticalTcbInventory::default();
    let root_control_resource = critical_resource(ROOT_CONTROL_ID)?;
    let root_control_task = temporal_task(ROOT_CONTROL_ID)?;
    let root_control_standard = mint_root_badged_cap(
        env,
        fault_endpoint,
        fault_sender_rights(),
        critical_standard_badge(ROOT_CONTROL_ID)?,
        "critical.root-control-fault",
    )?;
    let root_control_timeout = mint_root_badged_cap(
        env,
        fault_endpoint,
        fault_sender_rights(),
        root_control_task.timeout_badge,
        "critical.root-control-timeout",
    )?;
    let root_control_reply = env
        .alloc_reply()
        .map_err(|error| sel4_error("critical.root-control-reply", error))?;
    install_permanent_cnode_retention(
        env,
        root_control_resource,
        sel4_sys::seL4_CapInitThreadCNode,
    )?;
    inventory.register(CriticalTcbHandle {
        id: ROOT_CONTROL_ID,
        origin: CriticalTcbOrigin::InitRootControl,
        tcb_cap: sel4_sys::seL4_CapInitThreadTCB as usize,
        cnode_cap: sel4_sys::seL4_CapInitThreadCNode as usize,
        sched_context_cap: sel4_sys::seL4_CapInitThreadSC as usize,
        sched_control_cap: env
            .sched_control_for_core(root_control_task.core)
            .map_err(|error| sel4_error("critical.root-control-sched-control", error))?
            as usize,
        fault_endpoint_cap: root_control_standard as usize,
        timeout_endpoint_cap: root_control_timeout as usize,
        reply_cap: root_control_reply as usize,
        wake_notification_cap: root_control_wake as usize,
        revoke_anchor_cap: root_control_resource.revoke_anchor_slot as usize,
        core: root_control_task.core,
    })?;

    let mut children = Vec::<CriticalChildBacking, RESTRICTED_CHILD_COUNT>::new();
    let mut root_fault_reply = None;
    let mut root_emergency_reply = None;

    for id in [
        ROOT_FAULT_ID,
        ROOT_EMERGENCY_ID,
        WORKER_SUPERVISOR_ID,
        DRIVER_SUPERVISOR_ID,
        WORKER_EXECUTOR_GPU_ID,
        WORKER_EXECUTOR_LORA_ID,
    ] {
        let resource = critical_resource(id)?;
        let task = temporal_task(id)?;
        let entry = entrypoints
            .for_id(id)
            .ok_or(CriticalTcbConstructionError::InvalidEntrypoint)?;
        let wake = match id {
            ROOT_FAULT_ID => root_fault_wake,
            ROOT_EMERGENCY_ID => emergency_wake,
            WORKER_SUPERVISOR_ID => worker_wake,
            DRIVER_SUPERVISOR_ID => driver_wake,
            WORKER_EXECUTOR_GPU_ID => worker_executor_gpu_wake,
            WORKER_EXECUTOR_LORA_ID => worker_executor_lora_wake,
            _ => return Err(CriticalTcbConstructionError::MissingGeneratedRecord),
        };
        let child = construct_restricted_child(
            env,
            resource,
            task,
            entry,
            fault_endpoint,
            emergency_endpoint,
            wake,
            signals,
        )?;
        if id == ROOT_FAULT_ID {
            root_fault_reply = Some(child.handle.reply_cap as seL4_CPtr);
        } else if id == ROOT_EMERGENCY_ID {
            root_emergency_reply = Some(child.handle.reply_cap as seL4_CPtr);
        }
        inventory.register(child.handle)?;
        children
            .push(child.backing)
            .map_err(|_| CriticalTcbConstructionError::MissingGeneratedRecord)?;
    }

    let handles = inventory.finish()?;
    let root_fault_cnode = handles
        .iter()
        .find(|handle| handle.id == ROOT_FAULT_ID)
        .map(|handle| handle.cnode_cap)
        .filter(|cap| *cap != 0)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    TARGET_ROOT_FAULT_CNODE
        .compare_exchange(0, root_fault_cnode, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
    let driver_supervisor_cnode = handles
        .iter()
        .find(|handle| handle.id == DRIVER_SUPERVISOR_ID)
        .map(|handle| handle.cnode_cap)
        .filter(|cap| *cap != 0)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    TARGET_DRIVER_SUPERVISOR_CNODE
        .compare_exchange(
            0,
            driver_supervisor_cnode,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
    for (index, handle) in handles.iter().enumerate() {
        register_target_fault_source(
            handle.id,
            handle.tcb_cap as seL4_CPtr,
            GenerationIdentity {
                slot: u16::try_from(index)
                    .map_err(|_| CriticalTcbConstructionError::MissingGeneratedRecord)?,
                lease_epoch: 1,
                supervisor_generation: 1,
                cap_generation: 1,
            },
        )?;
    }
    Ok(CriticalTcbRuntime {
        handles,
        children,
        signals,
        faults: CriticalFaultCaps {
            fault_endpoint,
            emergency_endpoint,
            root_fault_reply: root_fault_reply
                .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?,
            root_emergency_reply: root_emergency_reply
                .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?,
        },
    })
}

fn target_fail_stop(reason: &'static str, emergency_cap: Option<seL4_CPtr>) -> ! {
    TARGET_FATAL.store(true, Ordering::Release);
    crate::debug_uart::debug_uart_line(reason);
    if let Some(cap) = emergency_cap.filter(|cap| *cap != sel4_sys::seL4_CapNull) {
        sel4::signal_unchecked(cap);
    }
    loop {
        sel4::yield_now();
    }
}

fn publish_target_worker_fault(record: FaultHandoffRecord) -> Result<(), FaultHandoffError> {
    let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
        return Err(FaultHandoffError::Contended);
    };
    handoff.publish_worker_fault(record)?;
    drop(handoff);
    sel4::signal_unchecked(CHILD_WORKER_SIGNAL_SLOT);
    Ok(())
}

fn publish_target_service_fault(record: FaultHandoffRecord) -> Result<(), FaultHandoffError> {
    let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
        return Err(FaultHandoffError::Contended);
    };
    handoff.publish_service_fault(record)
}

fn publish_target_driver_fault(record: FaultHandoffRecord) -> Result<(), FaultHandoffError> {
    let runtime_slot = record.identity.slot;
    let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
        return Err(FaultHandoffError::Contended);
    };
    handoff.publish_driver_fault(runtime_slot, record)?;
    drop(handoff);
    sel4::signal_unchecked(CHILD_DRIVER_SIGNAL_SLOT);
    Ok(())
}

fn resolve_target_fault(
    badge: seL4_Word,
) -> Result<(FaultRegistration, FaultClass), CriticalTcbConstructionError> {
    if !TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RegistryNotSealed);
    }
    let Some(registry) = TARGET_FAULT_REGISTRY.try_lock() else {
        return Err(CriticalTcbConstructionError::FaultHandoff(
            FaultHandoffError::Contended,
        ));
    };
    registry.resolve(badge).map_err(Into::into)
}

#[inline(never)]
fn recover_target_passive_service_call(
    record: FaultHandoffRecord,
) -> Result<(), CriticalTcbConstructionError> {
    let mailbox = service_fault_mailbox_index(record.task_index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let reply_slot = TARGET_SERVICE_RECOVERY_SLOTS[mailbox].load(Ordering::Acquire) as seL4_CPtr;
    if reply_slot == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let request_sequence = TARGET_SERVICE_CALL_SEQUENCES[mailbox].load(Ordering::Acquire);
    if request_sequence != 0 {
        TARGET_SERVICE_RECOVERY_REQUEST_SEQUENCES[mailbox]
            .store(request_sequence, Ordering::Relaxed);
        TARGET_SERVICE_RECOVERY_FAULT_SEQUENCES[mailbox].store(record.sequence, Ordering::Relaxed);
        TARGET_SERVICE_RECOVERY_FAULT_CLASSES[mailbox].store(
            match record.fault_class {
                FaultClass::Standard => 1,
                FaultClass::Timeout => 2,
            },
            Ordering::Relaxed,
        );
    }
    match TARGET_SERVICE_RECOVERY_STATES[mailbox].compare_exchange(
        SERVICE_RECOVERY_READY,
        SERVICE_RECOVERY_REPLIED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => {}
        Err(SERVICE_RECOVERY_REPLIED) => return Ok(()),
        Err(_) => return Err(CriticalTcbConstructionError::RuntimeNotReady),
    }
    let sequence = TARGET_SERVICE_CALL_SEQUENCES[mailbox].swap(0, Ordering::AcqRel);
    // A passive child can fault before its first receive or between calls. In
    // that case there is no blocked donor to release, but the transition to
    // REPLIED still permanently closes this generation and prevents a later
    // request from acquiring the stale Reply object.
    if sequence == 0 {
        return Ok(());
    }
    sel4::reply_to(
        reply_slot,
        sel4_sys::seL4_MessageInfo::new(
            secure9p_transport::NAMESPACE_REJECTED_LABEL as seL4_Word,
            0,
            0,
            2,
        ),
        [
            sequence as seL4_Word,
            secure9p_transport::TransportError::Closed.wire_code() as seL4_Word,
            0,
            0,
        ],
    );
    Ok(())
}

#[inline(never)]
fn recover_target_passive_worker_call(
    record: FaultHandoffRecord,
) -> Result<(), CriticalTcbConstructionError> {
    let worker_index = crate::critical_tcb::worker_fault_mailbox_index(record.task_index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let reply_slot =
        TARGET_WORKER_RECOVERY_SLOTS[worker_index].load(Ordering::Acquire) as seL4_CPtr;
    if reply_slot == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let request_sequence = TARGET_WORKER_CALL_SEQUENCES[worker_index].load(Ordering::Acquire);
    if request_sequence != 0 {
        TARGET_WORKER_RECOVERY_REQUEST_SEQUENCES[worker_index]
            .store(request_sequence, Ordering::Relaxed);
        TARGET_WORKER_RECOVERY_FAULT_SEQUENCES[worker_index]
            .store(record.sequence, Ordering::Relaxed);
        TARGET_WORKER_RECOVERY_FAULT_CLASSES[worker_index].store(
            match record.fault_class {
                FaultClass::Standard => 1,
                FaultClass::Timeout => 2,
            },
            Ordering::Relaxed,
        );
    }
    match TARGET_WORKER_RECOVERY_STATES[worker_index].compare_exchange(
        SERVICE_RECOVERY_READY,
        SERVICE_RECOVERY_REPLIED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => {}
        Err(SERVICE_RECOVERY_REPLIED) => return Ok(()),
        Err(_) => return Err(CriticalTcbConstructionError::RuntimeNotReady),
    }
    let sequence = TARGET_WORKER_CALL_SEQUENCES[worker_index].swap(0, Ordering::AcqRel);
    if sequence == 0 {
        return Ok(());
    }
    let status = if record.fault_class == FaultClass::Timeout {
        worker_task_abi::WorkerCompletionStatus::Timeout
    } else {
        worker_task_abi::WorkerCompletionStatus::Panic
    };
    sel4::reply_to(
        reply_slot,
        sel4_sys::seL4_MessageInfo::new(
            worker_task_abi::WORKER_CALL_RECOVERED_LABEL as seL4_Word,
            0,
            0,
            4,
        ),
        [
            sequence as seL4_Word,
            status as seL4_Word,
            TARGET_WORKER_CALL_SUPERVISOR_GENERATIONS[worker_index].load(Ordering::Acquire)
                as seL4_Word,
            TARGET_WORKER_CALL_CAP_GENERATIONS[worker_index].load(Ordering::Acquire) as seL4_Word,
        ],
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultReplyDisposition {
    Released,
    RetainedByDriver,
    CriticalTerminal { task_index: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingTargetFault {
    fault_label: seL4_Word,
    fault_badge: seL4_Word,
    fault_length: u16,
    fault_mr0: seL4_Word,
    fault_mr1: seL4_Word,
}

#[inline(never)]
fn publish_pending_target_fault(
    pending: PendingTargetFault,
) -> Result<(), CriticalTcbConstructionError> {
    if TARGET_PENDING_FAULT_VALID.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    TARGET_PENDING_FAULT_LABEL.store(pending.fault_label, Ordering::Relaxed);
    TARGET_PENDING_FAULT_BADGE.store(pending.fault_badge, Ordering::Relaxed);
    TARGET_PENDING_FAULT_LENGTH.store(usize::from(pending.fault_length), Ordering::Relaxed);
    TARGET_PENDING_FAULT_MR0.store(pending.fault_mr0, Ordering::Relaxed);
    TARGET_PENDING_FAULT_MR1.store(pending.fault_mr1, Ordering::Relaxed);
    TARGET_PENDING_FAULT_VALID.store(true, Ordering::Release);
    Ok(())
}

#[inline(never)]
fn pending_target_fault() -> Option<PendingTargetFault> {
    if !TARGET_PENDING_FAULT_VALID.load(Ordering::Acquire) {
        return None;
    }
    Some(PendingTargetFault {
        fault_label: TARGET_PENDING_FAULT_LABEL.load(Ordering::Relaxed),
        fault_badge: TARGET_PENDING_FAULT_BADGE.load(Ordering::Relaxed),
        fault_length: TARGET_PENDING_FAULT_LENGTH.load(Ordering::Relaxed) as u16,
        fault_mr0: TARGET_PENDING_FAULT_MR0.load(Ordering::Relaxed),
        fault_mr1: TARGET_PENDING_FAULT_MR1.load(Ordering::Relaxed),
    })
}

#[inline(never)]
fn clear_pending_target_fault() {
    TARGET_PENDING_FAULT_VALID.store(false, Ordering::Release);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingTargetServiceFault {
    record: FaultHandoffRecord,
    fault_handler_tcb_cap: seL4_CPtr,
    recover_passive_call: bool,
}

#[inline(never)]
fn publish_pending_target_service_fault(
    pending: PendingTargetServiceFault,
) -> Result<(), CriticalTcbConstructionError> {
    if TARGET_ROOT_FAULT_SERVICE_VALID.load(Ordering::Acquire) {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let record = pending.record;
    TARGET_ROOT_FAULT_SERVICE_SEQUENCE.store(record.sequence, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_TASK.store(usize::from(record.task_index), Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_IDENTITY_SLOT
        .store(usize::from(record.identity.slot), Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_IDENTITY_LEASE
        .store(record.identity.lease_epoch as usize, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_IDENTITY_SUPERVISOR.store(
        record.identity.supervisor_generation as usize,
        Ordering::Relaxed,
    );
    TARGET_ROOT_FAULT_SERVICE_IDENTITY_CAP
        .store(record.identity.cap_generation as usize, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_BADGE.store(record.fault_badge, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_CLASS.store(
        match record.fault_class {
            FaultClass::Standard => 0,
            FaultClass::Timeout => 1,
        },
        Ordering::Relaxed,
    );
    TARGET_ROOT_FAULT_SERVICE_LABEL.store(record.fault_label, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_LENGTH.store(usize::from(record.fault_length), Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_MR0.store(record.fault_mr0, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_MR1.store(record.fault_mr1, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_ROOT_TCB.store(record.tcb_cap, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_HANDLER_TCB
        .store(pending.fault_handler_tcb_cap as usize, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_RECOVER_PASSIVE
        .store(pending.recover_passive_call, Ordering::Relaxed);
    TARGET_ROOT_FAULT_SERVICE_VALID.store(true, Ordering::Release);
    Ok(())
}

#[inline(never)]
fn pending_target_service_fault() -> Option<PendingTargetServiceFault> {
    if !TARGET_ROOT_FAULT_SERVICE_VALID.load(Ordering::Acquire) {
        return None;
    }
    Some(PendingTargetServiceFault {
        record: FaultHandoffRecord {
            sequence: TARGET_ROOT_FAULT_SERVICE_SEQUENCE.load(Ordering::Relaxed),
            task_index: TARGET_ROOT_FAULT_SERVICE_TASK.load(Ordering::Relaxed) as u16,
            identity: GenerationIdentity {
                slot: TARGET_ROOT_FAULT_SERVICE_IDENTITY_SLOT.load(Ordering::Relaxed) as u16,
                lease_epoch: TARGET_ROOT_FAULT_SERVICE_IDENTITY_LEASE.load(Ordering::Relaxed)
                    as u32,
                supervisor_generation: TARGET_ROOT_FAULT_SERVICE_IDENTITY_SUPERVISOR
                    .load(Ordering::Relaxed) as u32,
                cap_generation: TARGET_ROOT_FAULT_SERVICE_IDENTITY_CAP.load(Ordering::Relaxed)
                    as u32,
            },
            fault_badge: TARGET_ROOT_FAULT_SERVICE_BADGE.load(Ordering::Relaxed),
            fault_class: match TARGET_ROOT_FAULT_SERVICE_CLASS.load(Ordering::Relaxed) {
                0 => FaultClass::Standard,
                1 => FaultClass::Timeout,
                _ => return None,
            },
            fault_label: TARGET_ROOT_FAULT_SERVICE_LABEL.load(Ordering::Relaxed),
            fault_length: TARGET_ROOT_FAULT_SERVICE_LENGTH.load(Ordering::Relaxed) as u16,
            fault_mr0: TARGET_ROOT_FAULT_SERVICE_MR0.load(Ordering::Relaxed),
            fault_mr1: TARGET_ROOT_FAULT_SERVICE_MR1.load(Ordering::Relaxed),
            tcb_cap: TARGET_ROOT_FAULT_SERVICE_ROOT_TCB.load(Ordering::Relaxed),
        },
        fault_handler_tcb_cap: TARGET_ROOT_FAULT_SERVICE_HANDLER_TCB.load(Ordering::Relaxed)
            as seL4_CPtr,
        recover_passive_call: TARGET_ROOT_FAULT_SERVICE_RECOVER_PASSIVE.load(Ordering::Relaxed),
    })
}

#[inline(never)]
fn clear_pending_target_service_fault() {
    TARGET_ROOT_FAULT_SERVICE_VALID.store(false, Ordering::Release);
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootFaultCriticalTurn {
    PrimeReceive = 0,
    Receive = 1,
    Classify = 2,
    ResolveService = 3,
    SuspendService = 4,
    RecoverPassiveService = 5,
    PublishService = 6,
    SuspendCritical = 7,
    SignalEmergency = 8,
}

#[inline(never)]
fn commit_root_fault_turn(turn: RootFaultCriticalTurn) {
    TARGET_ROOT_FAULT_TURN.store(turn as usize, Ordering::Release);
}

#[inline(never)]
fn commit_root_fault_suspend(task_index: u16) {
    TARGET_ROOT_FAULT_CRITICAL_TASK.store(usize::from(task_index), Ordering::Relaxed);
    TARGET_ROOT_FAULT_TURN.store(
        RootFaultCriticalTurn::SuspendCritical as usize,
        Ordering::Release,
    );
}

fn current_root_fault_turn() -> RootFaultCriticalTurn {
    match TARGET_ROOT_FAULT_TURN.load(Ordering::Acquire) {
        0 => RootFaultCriticalTurn::PrimeReceive,
        1 => RootFaultCriticalTurn::Receive,
        2 => RootFaultCriticalTurn::Classify,
        3 => RootFaultCriticalTurn::ResolveService,
        4 => RootFaultCriticalTurn::SuspendService,
        5 => RootFaultCriticalTurn::RecoverPassiveService,
        6 => RootFaultCriticalTurn::PublishService,
        7 => RootFaultCriticalTurn::SuspendCritical,
        8 => RootFaultCriticalTurn::SignalEmergency,
        _ => target_fail_stop(
            "[critical] root-fault cursor invalid",
            Some(CHILD_EMERGENCY_SIGNAL_SLOT),
        ),
    }
}

#[inline(always)]
fn is_generated_service_fault_badge(badge: seL4_Word) -> bool {
    let ninedoor = generated::ninedoor_service_config();
    let console = generated::console_network_service_config();
    (ninedoor.enabled && (badge == ninedoor.fault_badge || badge == ninedoor.timeout_badge))
        || (console.enabled && (badge == console.fault_badge || badge == console.timeout_badge))
}

#[inline(never)]
fn prepare_target_service_fault(
    fault_label: seL4_Word,
    badge: seL4_Word,
    fault_length: u16,
    fault_mr0: seL4_Word,
    fault_mr1: seL4_Word,
) -> Result<PendingTargetServiceFault, CriticalTcbConstructionError> {
    let (registration, fault_class) = resolve_target_fault(badge)?;
    let task = generated::temporal_tasks()
        .get(usize::from(registration.task_index))
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let timeout_label = sel4_sys::SEL4_MCS_FAULT_TIMEOUT_LABEL as seL4_Word;
    if !is_generated_service_fault_badge(badge)
        || !matches!(
            task.kind,
            TemporalTaskKind::Service | TemporalTaskKind::Drain
        )
        || (fault_class == FaultClass::Timeout) != (fault_label == timeout_label)
    {
        return Err(CriticalTcbConstructionError::FaultRegistry(
            FaultRegistryError::UnknownBadge,
        ));
    }
    let sequence = TARGET_FAULT_SEQUENCE.fetch_add(1, Ordering::AcqRel);
    if sequence == 0 {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    Ok(PendingTargetServiceFault {
        record: FaultHandoffRecord {
            sequence,
            task_index: registration.task_index,
            identity: registration.identity,
            fault_badge: badge,
            fault_class,
            fault_label,
            fault_length,
            fault_mr0,
            fault_mr1,
            tcb_cap: registration.tcb_cap,
        },
        fault_handler_tcb_cap: root_fault_tcb_control_cap(registration.task_index)?,
        recover_passive_call: task.kind == TemporalTaskKind::Service
            && task.execution == TemporalExecution::Passive
            && task.timeout_policy == TimeoutPolicy::ReturnError,
    })
}

fn handle_target_fault(
    fault_label: seL4_Word,
    badge: seL4_Word,
    fault_length: u16,
    fault_mr0: seL4_Word,
    fault_mr1: seL4_Word,
) -> Result<FaultReplyDisposition, CriticalTcbConstructionError> {
    let (registration, fault_class) = resolve_target_fault(badge)?;
    let task = generated::temporal_tasks()
        .get(usize::from(registration.task_index))
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let timeout_label = sel4_sys::SEL4_MCS_FAULT_TIMEOUT_LABEL as seL4_Word;
    if (fault_class == FaultClass::Timeout) != (fault_label == timeout_label) {
        return Err(CriticalTcbConstructionError::FaultRegistry(
            FaultRegistryError::UnknownBadge,
        ));
    }

    if fault_class == FaultClass::Timeout && task.timeout_policy == TimeoutPolicy::ReplenishOnce {
        let bit = 1u64
            .checked_shl(u32::from(registration.task_index))
            .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
        if TARGET_RECOVERED_TIMEOUTS.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
            sel4::reply_to(
                CHILD_REPLY_SLOT,
                sel4_sys::seL4_MessageInfo::new(0, 0, 0, 0),
                [0; 4],
            );
            return Ok(FaultReplyDisposition::Released);
        }
    }

    let sequence = TARGET_FAULT_SEQUENCE.fetch_add(1, Ordering::AcqRel);
    if sequence == 0 {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    let record = FaultHandoffRecord {
        sequence,
        task_index: registration.task_index,
        identity: registration.identity,
        fault_badge: badge,
        fault_class,
        fault_label,
        fault_length,
        fault_mr0,
        fault_mr1,
        tcb_cap: registration.tcb_cap,
    };
    let fault_handler_tcb_cap = root_fault_tcb_control_cap(registration.task_index)?;
    let disposition = match task.kind {
        TemporalTaskKind::Worker => {
            sel4::suspend_tcb(fault_handler_tcb_cap)
                .map_err(|error| sel4_error("critical.root-fault-worker-suspend", error))?;
            recover_target_passive_worker_call(record)?;
            publish_target_worker_fault(record)?;
            FaultReplyDisposition::Released
        }
        TemporalTaskKind::Driver => {
            DRIVER_FAULT_REPLY_BUSY
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
            publish_target_driver_fault(record)?;
            FaultReplyDisposition::RetainedByDriver
        }
        TemporalTaskKind::RootControl
        | TemporalTaskKind::RootFault
        | TemporalTaskKind::RootEmergency
        | TemporalTaskKind::WorkerSupervisor
        | TemporalTaskKind::DriverSupervisor
        | TemporalTaskKind::WorkerExecutor => FaultReplyDisposition::CriticalTerminal {
            task_index: registration.task_index,
        },
        // Exact generated service badges are routed through the persistent V6
        // cursor before this composite legacy classifier can consume them.
        TemporalTaskKind::Service | TemporalTaskKind::Drain => {
            return Err(CriticalTcbConstructionError::RuntimeNotReady)
        }
    };
    Ok(disposition)
}

extern "C" fn root_fault_entry(_arg0: seL4_Word) -> ! {
    if !TARGET_FAULT_REGISTRY_SEALED.load(Ordering::Acquire) {
        target_fail_stop(
            "[critical] root-fault started before exact registry seal",
            Some(CHILD_EMERGENCY_SIGNAL_SLOT),
        );
    }
    let release_badge = generated::worker_resource_admission_config()
        .handoff
        .root_fault_release_badge;
    loop {
        match current_root_fault_turn() {
            RootFaultCriticalTurn::PrimeReceive => {
                // The first fault receive starts only after an explicit MCS
                // replenishment boundary. No receive, Reply association, or
                // copied fault value exists across this one-time yield.
                commit_root_fault_turn(RootFaultCriticalTurn::Receive);
                sel4::yield_now();
            }
            RootFaultCriticalTurn::Receive => {
                commit_root_fault_turn(RootFaultCriticalTurn::Classify);
                #[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
                cohesix_root_fault_qemu_evidence_turn();
                let mut badge = 0;
                let (info, message_registers) =
                    sel4::recv_with_reply(CHILD_INBOX_SLOT, &mut badge, CHILD_REPLY_SLOT);
                let fault_length = info.length().min(seL4_Word::from(u16::MAX)) as u16;
                // Only copied message values cross this refill boundary. The
                // single Reply object stays in its fixed child CSpace slot,
                // and no second receive can replace its association before
                // the Classify turn consumes these values.
                if publish_pending_target_fault(PendingTargetFault {
                    fault_label: info.label(),
                    fault_badge: badge,
                    fault_length,
                    fault_mr0: if fault_length > 0 {
                        message_registers[0]
                    } else {
                        0
                    },
                    fault_mr1: if fault_length > 1 {
                        message_registers[1]
                    } else {
                        0
                    },
                })
                .is_err()
                {
                    target_fail_stop(
                        "[critical] root-fault publication state invalid",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    );
                }
                sel4::yield_now();
            }
            RootFaultCriticalTurn::Classify => {
                let PendingTargetFault {
                    fault_label,
                    fault_badge,
                    fault_length,
                    fault_mr0,
                    fault_mr1,
                } = match pending_target_fault() {
                    Some(fault) => fault,
                    None => target_fail_stop(
                        "[critical] root-fault classification state missing",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    ),
                };
                if is_generated_service_fault_badge(fault_badge) {
                    commit_root_fault_turn(RootFaultCriticalTurn::ResolveService);
                    sel4::yield_now();
                } else {
                    clear_pending_target_fault();
                    let disposition = match handle_target_fault(
                        fault_label,
                        fault_badge,
                        fault_length,
                        fault_mr0,
                        fault_mr1,
                    ) {
                        Ok(disposition) => disposition,
                        Err(_) => target_fail_stop(
                            "[critical] root-fault classification failed",
                            Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                        ),
                    };
                    match disposition {
                        FaultReplyDisposition::Released => {
                            commit_root_fault_turn(RootFaultCriticalTurn::Receive);
                            sel4::yield_now();
                        }
                        FaultReplyDisposition::RetainedByDriver => {
                            let mut observed_badge = 0;
                            let _ = sel4::wait(CHILD_DRIVER_RELEASE_SLOT, &mut observed_badge);
                            if observed_badge != release_badge
                                || DRIVER_FAULT_REPLY_BUSY.load(Ordering::Acquire)
                            {
                                target_fail_stop(
                                    "[critical] root-fault driver release invalid",
                                    Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                                );
                            }
                            commit_root_fault_turn(RootFaultCriticalTurn::Receive);
                            sel4::yield_now();
                        }
                        FaultReplyDisposition::CriticalTerminal { task_index } => {
                            commit_root_fault_suspend(task_index);
                            sel4::yield_now();
                        }
                    }
                }
            }
            RootFaultCriticalTurn::ResolveService => {
                let PendingTargetFault {
                    fault_label,
                    fault_badge,
                    fault_length,
                    fault_mr0,
                    fault_mr1,
                } = match pending_target_fault() {
                    Some(fault) => fault,
                    None => target_fail_stop(
                        "[critical] root-fault service classification state missing",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    ),
                };
                commit_root_fault_turn(RootFaultCriticalTurn::SuspendService);
                match prepare_target_service_fault(
                    fault_label,
                    fault_badge,
                    fault_length,
                    fault_mr0,
                    fault_mr1,
                ) {
                    Ok(pending) => {
                        if publish_pending_target_service_fault(pending).is_err() {
                            target_fail_stop(
                                "[critical] root-fault service snapshot invalid",
                                Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                            );
                        }
                        clear_pending_target_fault();
                    }
                    Err(CriticalTcbConstructionError::FaultHandoff(
                        FaultHandoffError::Contended,
                    )) => commit_root_fault_turn(RootFaultCriticalTurn::ResolveService),
                    Err(_) => target_fail_stop(
                        "[critical] root-fault service resolution failed",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    ),
                }
                sel4::yield_now();
            }
            RootFaultCriticalTurn::SuspendService => {
                let pending = match pending_target_service_fault() {
                    Some(pending) => pending,
                    None => target_fail_stop(
                        "[critical] root-fault service snapshot missing",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    ),
                };
                let successor = if pending.recover_passive_call {
                    RootFaultCriticalTurn::RecoverPassiveService
                } else {
                    RootFaultCriticalTurn::PublishService
                };
                commit_root_fault_turn(successor);
                if sel4::suspend_tcb_bounded(pending.fault_handler_tcb_cap).is_err() {
                    commit_root_fault_turn(RootFaultCriticalTurn::SuspendService);
                }
                sel4::yield_now();
            }
            RootFaultCriticalTurn::RecoverPassiveService => {
                let pending = match pending_target_service_fault() {
                    Some(pending) if pending.recover_passive_call => pending,
                    _ => target_fail_stop(
                        "[critical] root-fault passive recovery state invalid",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    ),
                };
                commit_root_fault_turn(RootFaultCriticalTurn::PublishService);
                if recover_target_passive_service_call(pending.record).is_err() {
                    commit_root_fault_turn(RootFaultCriticalTurn::RecoverPassiveService);
                }
                sel4::yield_now();
            }
            RootFaultCriticalTurn::PublishService => {
                let pending = match pending_target_service_fault() {
                    Some(pending) => pending,
                    None => target_fail_stop(
                        "[critical] root-fault service publication state missing",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    ),
                };
                commit_root_fault_turn(RootFaultCriticalTurn::Receive);
                if publish_target_service_fault(pending.record).is_ok() {
                    clear_pending_target_service_fault();
                } else {
                    commit_root_fault_turn(RootFaultCriticalTurn::PublishService);
                }
                sel4::yield_now();
            }
            RootFaultCriticalTurn::SuspendCritical => {
                commit_root_fault_turn(RootFaultCriticalTurn::SignalEmergency);
                let task_index = TARGET_ROOT_FAULT_CRITICAL_TASK.load(Ordering::Acquire) as u16;
                let fault_handler_tcb_cap = match root_fault_tcb_control_cap(task_index) {
                    Ok(cap) => cap,
                    Err(_) => target_fail_stop(
                        "[critical] root-fault critical TCB cap missing",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    ),
                };
                if sel4::suspend_tcb(fault_handler_tcb_cap).is_err() {
                    target_fail_stop(
                        "[critical] root-fault critical suspend failed",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    );
                }
                sel4::yield_now();
            }
            RootFaultCriticalTurn::SignalEmergency => {
                commit_root_fault_turn(RootFaultCriticalTurn::Receive);
                sel4::signal_unchecked(CHILD_EMERGENCY_SIGNAL_SLOT);
                sel4::yield_now();
            }
        }
    }
}

extern "C" fn root_emergency_entry(_arg0: seL4_Word) -> ! {
    #[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
    cohesix_root_emergency_qemu_evidence_wait();
    let mut badge = 0;
    let _ = sel4::recv_with_reply(CHILD_INBOX_SLOT, &mut badge, CHILD_REPLY_SLOT);
    target_fail_stop("[critical] root-emergency fail-stop", None)
}

extern "C" fn root_worker_supervisor_entry(_arg0: seL4_Word) -> ! {
    if !crate::worker_supervisor::target_supervisor_startup_ready() {
        target_fail_stop(
            "[critical] Worker supervisor backend missing",
            Some(CHILD_EMERGENCY_SIGNAL_SLOT),
        );
    }
    loop {
        #[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
        cohesix_worker_supervisor_qemu_evidence_wait();
        let mut badge = 0;
        let _ = sel4::wait(CHILD_INBOX_SLOT, &mut badge);
        let Some(drain_critical_handoff) = validate_worker_supervisor_wake(badge) else {
            target_fail_stop(
                "[critical] Worker supervisor wake badge invalid",
                Some(CHILD_EMERGENCY_SIGNAL_SLOT),
            );
        };
        if crate::worker_supervisor::drain_target_wake(badge).is_err() {
            target_fail_stop(
                "[critical] Worker supervisor child wake failed",
                Some(CHILD_EMERGENCY_SIGNAL_SLOT),
            );
        }
        if !drain_critical_handoff {
            continue;
        }
        for _ in 0..(WORKER_FAULT_MAILBOX_CAPACITY + WORKER_CONTROL_QUEUE_CAPACITY) {
            let item = {
                let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
                    // Every producer publishes before signalling. If one owns
                    // the lock now, its wake remains pending for the next
                    // bounded turn; transient producer contention is not data
                    // loss and must not convert normal coalescence into fatal.
                    break;
                };
                let fault = handoff.drain_worker_fault();
                drop(handoff);
                match fault {
                    Some(record) => Some(WorkerSupervisorItem::Fault(record)),
                    None => match TARGET_WORKER_CONTROL.validate_next() {
                        Ok(Some(record)) => Some(WorkerSupervisorItem::Control(record)),
                        Ok(None) => None,
                        Err(_) => target_fail_stop(
                            "[critical] Worker control ring ownership failed",
                            Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                        ),
                    },
                }
            };
            let Some(item) = item else {
                break;
            };
            let result = match item {
                WorkerSupervisorItem::Fault(record) => {
                    crate::worker_supervisor::drain_critical_fault(record)
                }
                WorkerSupervisorItem::Control(_) => Ok(()),
            };
            if result.is_err() {
                target_fail_stop(
                    "[critical] Worker supervisor containment failed",
                    Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                );
            }
        }
        let retry = match TARGET_HANDOFF.try_lock() {
            Some(handoff) => {
                handoff.worker_fault_pending() || TARGET_WORKER_CONTROL.validation_pending()
            }
            None => true,
        };
        if retry {
            // This Write-only self-wake covers contention with the other
            // supervisor as well as a producer. It carries no authority beyond
            // scheduling another bounded drain of this same notification.
            sel4::signal_unchecked(CHILD_WORKER_SIGNAL_SLOT);
        }
    }
}

extern "C" fn root_worker_executor_gpu_entry(_arg0: seL4_Word) -> ! {
    crate::hal::worker_task::run_target_worker_executor(
        crate::hal::worker_task::TargetWorkerExecutorLane::Gpu,
    )
}

extern "C" fn root_worker_executor_lora_entry(_arg0: seL4_Word) -> ! {
    crate::hal::worker_task::run_target_worker_executor(
        crate::hal::worker_task::TargetWorkerExecutorLane::Lora,
    )
}

extern "C" fn root_driver_supervisor_entry(ipc_buffer_vaddr: seL4_Word) -> ! {
    let expected_badge = generated::worker_resource_admission_config()
        .handoff
        .driver_wake_badge;
    let mut deferred_record: Option<FaultHandoffRecord> = None;
    loop {
        #[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
        cohesix_driver_supervisor_qemu_evidence_wait();
        let mut badge = 0;
        let _ = sel4::wait(CHILD_INBOX_SLOT, &mut badge);
        if badge != expected_badge {
            target_fail_stop(
                "[critical] driver supervisor wake badge invalid",
                Some(CHILD_EMERGENCY_SIGNAL_SLOT),
            );
        }
        for _ in 0..DRIVER_FAULT_RECORD_CAPACITY {
            let record = match deferred_record.take() {
                Some(record) => Some(record),
                None => {
                    let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
                        // Publication precedes the wake, so a racing producer
                        // leaves a notification pending for the next bounded turn.
                        break;
                    };
                    handoff.drain_driver()
                }
            };
            let Some(record) = record else {
                break;
            };
            match crate::hal::driver_task::root_driver_supervisor_contain_fault(
                record,
                ipc_buffer_vaddr as usize,
            ) {
                Ok(()) => {}
                Err(
                    crate::hal::driver_task::DriverSupervisorContainmentError::RootProducerActive,
                ) => {
                    // Admission and publication are already fenced. Preserve
                    // this exact record in the supervisor's own stack and
                    // retry on another bounded notification turn after the
                    // root producer observes closure and drops its guard.
                    deferred_record = Some(record);
                    sel4::signal_unchecked(CHILD_DRIVER_SIGNAL_SLOT);
                    break;
                }
                Err(_) => {
                    target_fail_stop(
                        "[critical] driver supervisor containment failed",
                        Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                    );
                }
            }
            if DRIVER_FAULT_REPLY_BUSY
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                target_fail_stop(
                    "[critical] driver supervisor Reply release invalid",
                    Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                );
            }
            sel4::signal_unchecked(CHILD_DRIVER_RELEASE_SIGNAL_SLOT);
        }
        let retry = match TARGET_HANDOFF.try_lock() {
            Some(handoff) => handoff.driver_pending(),
            None => true,
        };
        if retry {
            sel4::signal_unchecked(CHILD_DRIVER_SIGNAL_SLOT);
        }
    }
}

/// Stable external-QEMU observation point before one blocking root-fault receive.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_root_fault_qemu_evidence_turn() {
    core::hint::black_box(0x26e0_0001_u32);
}

/// Stable external-QEMU observation point before root-emergency blocks.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_root_emergency_qemu_evidence_wait() {
    core::hint::black_box(0x26e0_0002_u32);
}

/// Stable external-QEMU observation point before the Worker supervisor waits.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_worker_supervisor_qemu_evidence_wait() {
    core::hint::black_box(0x26e0_0003_u32);
}

/// Stable external-QEMU observation point before the driver supervisor waits.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_driver_supervisor_qemu_evidence_wait() {
    core::hint::black_box(0x26e0_0004_u32);
}

struct ConstructedChild {
    handle: CriticalTcbHandle,
    backing: CriticalChildBacking,
}

#[allow(clippy::too_many_arguments)]
fn construct_restricted_child(
    env: &mut KernelEnv<'_>,
    resource: &CriticalTcbResource,
    task: &TemporalTaskConfig,
    entry: usize,
    fault_endpoint: seL4_CPtr,
    emergency_endpoint: seL4_CPtr,
    wake_notification: seL4_CPtr,
    signals: CriticalSignalCaps,
) -> Result<ConstructedChild, CriticalTcbConstructionError> {
    let child_depth = resource.cnode_radix_bits;
    let root_depth = sel4::word_bits() as u8;
    let root_cnode = env.init_cnode_cap();
    let cnode = env
        .alloc_cnode(child_depth)
        .map_err(|error| sel4_error("critical.child-cnode", error))?;
    let tcb = env
        .alloc_tcb()
        .map_err(|error| sel4_error("critical.child-tcb", error))?;
    let sched_context = env
        .alloc_sched_context(task.scheduling_context_bits)
        .map_err(|error| sel4_error("critical.child-sc", error))?;
    let reply = env
        .alloc_reply()
        .map_err(|error| sel4_error("critical.child-reply", error))?;

    let mut ipc_frame = env
        .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
        .map_err(|error| sel4_error("critical.child-ipc-frame", error))?;
    ipc_frame.as_mut_slice().fill(0);
    let (stack_frames, stack_bottom, stack_top) = allocate_stack(env, resource.stack_pages)?;

    let (fault_source, fault_badge, timeout_source) = if task.id == ROOT_EMERGENCY_ID {
        (sel4_sys::seL4_CapNull, 0, sel4_sys::seL4_CapNull)
    } else if task.id == ROOT_FAULT_ID {
        (
            emergency_endpoint,
            critical_standard_badge(task.id)?,
            emergency_endpoint,
        )
    } else {
        (
            fault_endpoint,
            critical_standard_badge(task.id)?,
            fault_endpoint,
        )
    };
    let root_fault_cap = if fault_source == sel4_sys::seL4_CapNull {
        sel4_sys::seL4_CapNull
    } else {
        mint_root_badged_cap(
            env,
            fault_source,
            fault_sender_rights(),
            fault_badge,
            "critical.child-standard-fault",
        )?
    };
    let root_timeout_cap = if timeout_source == sel4_sys::seL4_CapNull {
        sel4_sys::seL4_CapNull
    } else {
        mint_root_badged_cap(
            env,
            timeout_source,
            fault_sender_rights(),
            task.timeout_badge,
            "critical.child-timeout-fault",
        )?
    };

    if root_fault_cap != sel4_sys::seL4_CapNull {
        copy_child_cap(
            cnode,
            child_depth,
            CHILD_STANDARD_FAULT_SLOT,
            root_cnode,
            root_fault_cap,
            root_depth,
            "critical.child-standard-fault-slot",
        )?;
        copy_child_cap(
            cnode,
            child_depth,
            CHILD_TIMEOUT_FAULT_SLOT,
            root_cnode,
            root_timeout_cap,
            root_depth,
            "critical.child-timeout-fault-slot",
        )?;
    }

    let inbox_source = match task.id {
        ROOT_FAULT_ID => fault_endpoint,
        ROOT_EMERGENCY_ID => emergency_endpoint,
        WORKER_SUPERVISOR_ID
        | DRIVER_SUPERVISOR_ID
        | WORKER_EXECUTOR_GPU_ID
        | WORKER_EXECUTOR_LORA_ID => wake_notification,
        _ => return Err(CriticalTcbConstructionError::MissingGeneratedRecord),
    };
    mint_child_cap(
        cnode,
        child_depth,
        CHILD_INBOX_SLOT,
        root_cnode,
        inbox_source,
        root_depth,
        read_only_rights(),
        0,
        "critical.child-inbox",
    )?;
    if task.id == ROOT_FAULT_ID {
        mint_child_cap(
            cnode,
            child_depth,
            CHILD_DRIVER_RELEASE_SLOT,
            root_cnode,
            wake_notification,
            root_depth,
            read_only_rights(),
            0,
            "critical.root-fault-release-wait",
        )?;
        for (slot, source, stage) in [
            (
                CHILD_WORKER_SIGNAL_SLOT,
                signals.worker_supervisor,
                "critical.root-fault-worker-signal",
            ),
            (
                CHILD_DRIVER_SIGNAL_SLOT,
                signals.driver_supervisor,
                "critical.root-fault-driver-signal",
            ),
            (
                CHILD_EMERGENCY_SIGNAL_SLOT,
                signals.emergency,
                "critical.root-fault-emergency-signal",
            ),
        ] {
            copy_child_cap(
                cnode,
                child_depth,
                slot,
                root_cnode,
                source,
                root_depth,
                stage,
            )?;
        }
    } else if matches!(task.id, WORKER_SUPERVISOR_ID | DRIVER_SUPERVISOR_ID) {
        copy_child_cap(
            cnode,
            child_depth,
            CHILD_EMERGENCY_SIGNAL_SLOT,
            root_cnode,
            signals.emergency,
            root_depth,
            "critical.supervisor-emergency-signal",
        )?;
        let (self_signal_slot, self_signal, stage) = if task.id == WORKER_SUPERVISOR_ID {
            (
                CHILD_WORKER_SIGNAL_SLOT,
                signals.worker_supervisor,
                "critical.worker-supervisor-self-signal",
            )
        } else {
            (
                CHILD_DRIVER_SIGNAL_SLOT,
                signals.driver_supervisor,
                "critical.driver-supervisor-self-signal",
            )
        };
        copy_child_cap(
            cnode,
            child_depth,
            self_signal_slot,
            root_cnode,
            self_signal,
            root_depth,
            stage,
        )?;
        if task.id == DRIVER_SUPERVISOR_ID {
            copy_child_cap(
                cnode,
                child_depth,
                CHILD_DRIVER_RELEASE_SIGNAL_SLOT,
                root_cnode,
                signals.root_fault_release,
                root_depth,
                "critical.driver-supervisor-root-fault-release",
            )?;
            copy_child_cap(
                cnode,
                child_depth,
                CHILD_SELF_CNODE_SLOT,
                root_cnode,
                cnode,
                root_depth,
                "critical.driver-supervisor-self-cnode",
            )?;
        }
    } else if matches!(task.id, WORKER_EXECUTOR_GPU_ID | WORKER_EXECUTOR_LORA_ID) {
        let completion_signal = if task.id == WORKER_EXECUTOR_GPU_ID {
            signals.worker_executor_completion_gpu
        } else {
            signals.worker_executor_completion_lora
        };
        copy_child_cap(
            cnode,
            child_depth,
            CHILD_EXECUTOR_COMPLETION_SIGNAL_SLOT,
            root_cnode,
            completion_signal,
            root_depth,
            "critical.worker-executor-completion-signal",
        )?;
        mint_child_cap(
            cnode,
            child_depth,
            4,
            root_cnode,
            wake_notification,
            root_depth,
            write_only_rights(),
            1,
            "critical.worker-executor-self-signal",
        )?;
    }
    if !matches!(task.id, WORKER_EXECUTOR_GPU_ID | WORKER_EXECUTOR_LORA_ID) {
        copy_child_cap(
            cnode,
            child_depth,
            CHILD_REPLY_SLOT,
            root_cnode,
            reply,
            root_depth,
            "critical.child-reply-slot",
        )?;
    }

    let guard_bits = sel4::word_bits().saturating_sub(seL4_Word::from(child_depth));
    let cspace_root_data = sel4::cap_data_guard(0, guard_bits);
    // SetSpace resolves the fault endpoint in the calling root CSpace, not in
    // the child CNode being installed. The root-emergency lane deliberately
    // supplies CapNull; every other lane supplies its retained badged root cap.
    sel4::set_tcb_space(
        tcb,
        root_fault_cap,
        cnode,
        cspace_root_data,
        sel4_sys::seL4_CapInitThreadVSpace,
        0,
    )
    .map_err(|error| sel4_error("critical.child-space", error))?;
    let ipc_buffer_vaddr = ipc_frame.ptr().as_ptr() as usize;
    env.bind_child_ipc_buffer(tcb, ipc_frame.cap(), ipc_buffer_vaddr)
        .map_err(|error| sel4_error("critical.child-ipc-bind", error))?;
    configure_active_sc(
        env,
        task,
        tcb,
        sched_context,
        root_fault_cap,
        root_timeout_cap,
    )?;
    if matches!(
        task.id,
        ROOT_EMERGENCY_ID | WORKER_SUPERVISOR_ID | DRIVER_SUPERVISOR_ID
    ) {
        sel4::bind_tcb_notification(tcb, wake_notification)
            .map_err(|error| sel4_error("critical.child-notification-bind", error))?;
    }
    install_permanent_cnode_retention(env, resource, cnode)?;
    sel4::write_tcb_registers(tcb, entry, stack_top, ipc_buffer_vaddr as seL4_Word, false)
        .map_err(|error| sel4_error("critical.child-registers", error))?;

    Ok(ConstructedChild {
        handle: CriticalTcbHandle {
            id: task.id,
            origin: CriticalTcbOrigin::RestrictedChild,
            tcb_cap: tcb as usize,
            cnode_cap: cnode as usize,
            sched_context_cap: sched_context as usize,
            sched_control_cap: env
                .sched_control_for_core(task.core)
                .map_err(|error| sel4_error("critical.child-sched-control", error))?
                as usize,
            fault_endpoint_cap: root_fault_cap as usize,
            timeout_endpoint_cap: root_timeout_cap as usize,
            reply_cap: reply as usize,
            wake_notification_cap: wake_notification as usize,
            revoke_anchor_cap: resource.revoke_anchor_slot as usize,
            core: task.core,
        },
        backing: CriticalChildBacking {
            id: task.id,
            ipc_frame: ipc_frame.cap(),
            stack_frames,
            stack_bottom,
            stack_top,
        },
    })
}

fn configure_active_sc(
    env: &KernelEnv<'_>,
    task: &TemporalTaskConfig,
    tcb: seL4_CPtr,
    sched_context: seL4_CPtr,
    standard_fault_cap: seL4_CPtr,
    timeout_fault_cap: seL4_CPtr,
) -> Result<(), CriticalTcbConstructionError> {
    let sched_control = env
        .sched_control_for_core(task.core)
        .map_err(|error| sel4_error("critical.sched-control", error))?;
    configure_active_sc_with_sched_control(
        task,
        tcb,
        sched_context,
        sched_control,
        standard_fault_cap,
        timeout_fault_cap,
    )
}

fn configure_active_sc_with_sched_control(
    task: &TemporalTaskConfig,
    tcb: seL4_CPtr,
    sched_context: seL4_CPtr,
    sched_control: seL4_CPtr,
    standard_fault_cap: seL4_CPtr,
    timeout_fault_cap: seL4_CPtr,
) -> Result<(), CriticalTcbConstructionError> {
    let extra_refills = mcs_extra_refills(task.max_refills)?;
    sel4::configure_sched_context(
        sched_control,
        sched_context,
        u64::from(task.budget_us),
        u64::from(task.period_us),
        seL4_Word::from(extra_refills),
        task.timeout_badge,
        0,
    )
    .map_err(|error| sel4_error("critical.sc-configure", error))?;
    sel4::set_tcb_sched_params_mcs(
        tcb,
        sel4_sys::seL4_CapInitThreadTCB,
        task.mcp,
        task.priority,
        sched_context,
        standard_fault_cap,
    )
    .map_err(|error| sel4_error("critical.tcb-sched-params", error))?;
    if requires_timeout_endpoint(task.timeout_policy) && timeout_fault_cap != sel4_sys::seL4_CapNull
    {
        sel4::set_tcb_timeout_endpoint(tcb, timeout_fault_cap)
            .map_err(|error| sel4_error("critical.tcb-timeout-endpoint", error))?;
    }
    Ok(())
}

/// Whether the generated policy installs the separately reserved timeout cap.
///
/// Natural postponement leaves the timeout cap minted, registered, and
/// accounted, but lets seL4 advance an adjacent refill without converting that
/// ordinary replenishment boundary into a terminal fault. The standard fault
/// endpoint remains installed through `set_tcb_sched_params_mcs` above.
const fn requires_timeout_endpoint(policy: TimeoutPolicy) -> bool {
    !matches!(policy, TimeoutPolicy::NaturalPostpone)
}

fn allocate_stack(
    env: &mut KernelEnv<'_>,
    page_count: u8,
) -> Result<(Vec<seL4_CPtr, MAX_CRITICAL_STACK_PAGES>, usize, usize), CriticalTcbConstructionError>
{
    if page_count == 0 || usize::from(page_count) > MAX_CRITICAL_STACK_PAGES {
        return Err(CriticalTcbConstructionError::InvalidStackLayout);
    }
    let mut frames = Vec::<seL4_CPtr, MAX_CRITICAL_STACK_PAGES>::new();
    let mut first = None;
    let mut expected = None;
    let mut last_end = 0usize;
    for _ in 0..page_count {
        let mut frame: RamFrame = env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
            .map_err(|error| sel4_error("critical.child-stack-frame", error))?;
        frame.as_mut_slice().fill(0);
        let start = frame.ptr().as_ptr() as usize;
        if expected.is_some_and(|address| address != start) {
            return Err(CriticalTcbConstructionError::InvalidStackLayout);
        }
        first.get_or_insert(start);
        last_end = start
            .checked_add(1usize << sel4::PAGE_BITS)
            .ok_or(CriticalTcbConstructionError::InvalidStackLayout)?;
        expected = Some(last_end);
        frames
            .push(frame.cap())
            .map_err(|_| CriticalTcbConstructionError::InvalidStackLayout)?;
    }
    let bottom = first.ok_or(CriticalTcbConstructionError::InvalidStackLayout)?;
    Ok((frames, bottom, last_end & !0xf))
}

fn critical_resource(
    id: &str,
) -> Result<&'static CriticalTcbResource, CriticalTcbConstructionError> {
    generated::worker_resource_admission_config()
        .critical_tcbs
        .iter()
        .find(|resource| resource.id == id)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)
}

fn temporal_task(id: &str) -> Result<&'static TemporalTaskConfig, CriticalTcbConstructionError> {
    generated::temporal_tasks()
        .iter()
        .find(|task| task.id == id)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)
}

fn critical_standard_badge(id: &str) -> Result<u64, CriticalTcbConstructionError> {
    generated_standard_fault_badge(id).ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)
}

fn validate_generated_rights() -> Result<(), CriticalTcbConstructionError> {
    let admission = generated::worker_resource_admission_config();
    let handoff = admission.handoff;
    if handoff.supervisor_signal_rights
        != (generated::CapabilityRights {
            read: false,
            write: true,
            grant: false,
            grant_reply: false,
        })
        || handoff.supervisor_wait_rights
            != (generated::CapabilityRights {
                read: true,
                write: false,
                grant: false,
                grant_reply: false,
            })
        || handoff.fault_sender_rights
            != (generated::CapabilityRights {
                read: false,
                write: true,
                grant: false,
                grant_reply: true,
            })
        || handoff.fault_receiver_rights
            != (generated::CapabilityRights {
                read: true,
                write: false,
                grant: false,
                grant_reply: false,
            })
    {
        return Err(CriticalTcbConstructionError::InvalidCapabilityRights);
    }
    if handoff.driver_fault_records != 0 {
        let resource = admission
            .critical_tcbs
            .iter()
            .find(|resource| resource.id == DRIVER_SUPERVISOR_ID)
            .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
        let required_caps = 8usize
            .checked_add(
                usize::from(handoff.driver_fault_records)
                    .checked_mul(DRIVER_SUPERVISOR_RUNTIME_CAP_STRIDE as usize)
                    .ok_or(CriticalTcbConstructionError::InvalidCapabilityRights)?,
            )
            .ok_or(CriticalTcbConstructionError::InvalidCapabilityRights)?;
        let final_slot = driver_supervisor_runtime_cap_slot(
            handoff.driver_fault_records.saturating_sub(1),
            DRIVER_SUPERVISOR_RUNTIME_TIMEOUT_FAULT_OFFSET,
        )?;
        let child_slots = 1usize
            .checked_shl(u32::from(resource.cnode_radix_bits))
            .ok_or(CriticalTcbConstructionError::InvalidCapabilityRights)?;
        if usize::from(resource.cspace_cap_count) != required_caps
            || final_slot as usize >= child_slots
        {
            return Err(CriticalTcbConstructionError::InvalidCapabilityRights);
        }
    }
    Ok(())
}

fn install_permanent_cnode_retention(
    env: &KernelEnv<'_>,
    resource: &CriticalTcbResource,
    source: seL4_CPtr,
) -> Result<(), CriticalTcbConstructionError> {
    let retention_slot = seL4_CPtr::try_from(resource.revoke_anchor_slot)
        .map_err(|_| sel4_error("critical.retention-convert", sel4_sys::seL4_RangeError))?;
    let depth = sel4::word_bits() as u8;
    let error = sel4::cnode_copy_depth(
        env.init_cnode_cap(),
        retention_slot,
        depth,
        env.init_cnode_cap(),
        source,
        depth,
        sel4_sys::seL4_CapRights_All,
    );
    if error == sel4_sys::seL4_NoError {
        Ok(())
    } else {
        Err(sel4_error("critical.retention-copy", error))
    }
}

#[allow(clippy::too_many_arguments)]
fn mint_child_cap(
    destination_cnode: seL4_CPtr,
    destination_depth: u8,
    destination_slot: seL4_CPtr,
    source_cnode: seL4_CPtr,
    source_slot: seL4_CPtr,
    source_depth: u8,
    rights: sel4_sys::seL4_CapRights,
    badge: seL4_Word,
    stage: &'static str,
) -> Result<(), CriticalTcbConstructionError> {
    let error = sel4::cnode_mint_depth(
        destination_cnode,
        destination_slot,
        destination_depth,
        source_cnode,
        source_slot,
        source_depth,
        rights,
        badge,
    );
    if error == sel4_sys::seL4_NoError {
        Ok(())
    } else {
        Err(sel4_error(stage, error))
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_child_cap(
    destination_cnode: seL4_CPtr,
    destination_depth: u8,
    destination_slot: seL4_CPtr,
    source_cnode: seL4_CPtr,
    source_slot: seL4_CPtr,
    source_depth: u8,
    stage: &'static str,
) -> Result<(), CriticalTcbConstructionError> {
    let error = sel4::cnode_copy_depth(
        destination_cnode,
        destination_slot,
        destination_depth,
        source_cnode,
        source_slot,
        source_depth,
        sel4_sys::seL4_CapRights_All,
    );
    if error == sel4_sys::seL4_NoError {
        Ok(())
    } else {
        Err(sel4_error(stage, error))
    }
}

fn mint_root_badged_cap(
    env: &mut KernelEnv<'_>,
    source: seL4_CPtr,
    rights: sel4_sys::seL4_CapRights,
    badge: u64,
    stage: &'static str,
) -> Result<seL4_CPtr, CriticalTcbConstructionError> {
    let badge =
        seL4_Word::try_from(badge).map_err(|_| sel4_error(stage, sel4_sys::seL4_RangeError))?;
    let slot = env
        .try_allocate_slot()
        .map_err(|error| sel4_error(stage, error))?;
    let depth = sel4::word_bits() as u8;
    let error = sel4::cnode_mint_depth(
        env.init_cnode_cap(),
        slot,
        depth,
        env.init_cnode_cap(),
        source,
        depth,
        rights,
        badge,
    );
    if error == sel4_sys::seL4_NoError {
        Ok(slot)
    } else {
        Err(sel4_error(stage, error))
    }
}

const fn write_only_rights() -> sel4_sys::seL4_CapRights {
    sel4_sys::seL4_CapRights::new(0, 0, 0, 1)
}

const fn read_only_rights() -> sel4_sys::seL4_CapRights {
    sel4_sys::seL4_CapRights::new(0, 0, 1, 0)
}

const fn fault_sender_rights() -> sel4_sys::seL4_CapRights {
    sel4_sys::seL4_CapRights::new(1, 0, 0, 1)
}

const fn sel4_error(stage: &'static str, error: seL4_Error) -> CriticalTcbConstructionError {
    CriticalTcbConstructionError::Sel4 { stage, error }
}
