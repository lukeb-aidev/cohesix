// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Construct MCS critical root progress domains from compiler-owned temporal records.
// Author: Lukas Bower

//! HAL construction for the Milestone 26e critical root domains.
//!
//! The init TCB and initial SC are the real `root-control` domain because that
//! thread owns bootstrap and HAL admission.  This module does not create a
//! phantom control child.  It creates four restricted CSpaces/TCBs, binds one
//! independently configured active SC to each, and resumes only caller-supplied
//! entrypoints for fault, emergency, Worker-supervisor, and driver-supervisor
//! work.  All four children share the root VSpace deliberately so the small
//! root-resident entrypoints and HAL-mapped private stack/IPC pages are visible;
//! their capability views remain separate and compiler-bounded.

use crate::critical_tcb::{
    generated_standard_fault_badge, mcs_extra_refills, passive_service_recovery_contract,
    service_fault_mailbox_index, validate_critical_temporal_graph, validate_worker_supervisor_wake,
    CriticalHandoff, CriticalTcbHandle, CriticalTcbInventory, CriticalTcbOrigin,
    CriticalTopologyError, FaultClass, FaultHandoffError, FaultHandoffRecord, FaultRegistration,
    FaultRegistry, FaultRegistryError, GenerationIdentity, PublishResult, WorkerControlRecord,
    WorkerSupervisorItem, CRITICAL_TCB_COUNT, DRIVER_FAULT_RECORD_CAPACITY,
    FAULT_REGISTRY_CAPACITY, SERVICE_FAULT_RECORD_CAPACITY, WORKER_CONTROL_QUEUE_CAPACITY,
    WORKER_FAULT_MAILBOX_CAPACITY,
};
use crate::generated::{
    self, CriticalTcbResource, TemporalTaskConfig, TemporalTaskKind, TimeoutPolicy,
};
use crate::sel4::{self, KernelEnv, RamFrame};
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

const ROOT_CONTROL_ID: &str = "root-control";
const ROOT_FAULT_ID: &str = "root-fault";
const ROOT_EMERGENCY_ID: &str = "root-emergency";
const WORKER_SUPERVISOR_ID: &str = "root-worker-supervisor";
const DRIVER_SUPERVISOR_ID: &str = "root-driver-supervisor";

/// Concrete root-resident entrypoints for the four restricted critical children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CriticalTcbEntrypoints {
    pub root_fault: usize,
    pub root_emergency: usize,
    pub worker_supervisor: usize,
    pub driver_supervisor: usize,
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
        }
    }

    fn for_id(self, id: &str) -> Option<usize> {
        match id {
            ROOT_FAULT_ID => Some(self.root_fault),
            ROOT_EMERGENCY_ID => Some(self.root_emergency),
            WORKER_SUPERVISOR_ID => Some(self.worker_supervisor),
            DRIVER_SUPERVISOR_ID => Some(self.driver_supervisor),
            _ => None,
        }
    }

    fn validate(self) -> Result<(), CriticalTcbConstructionError> {
        for entry in [
            self.root_fault,
            self.root_emergency,
            self.worker_supervisor,
            self.driver_supervisor,
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
static TARGET_FAULT_REGISTRY: Mutex<FaultRegistry> = Mutex::new(FaultRegistry::new());
static TARGET_FAULT_REGISTRY_SEALED: AtomicBool = AtomicBool::new(false);
static TARGET_FAULT_RECEIVER_ACTIVE: AtomicBool = AtomicBool::new(false);
static TARGET_FATAL: AtomicBool = AtomicBool::new(false);
static TARGET_FAULT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static TARGET_RECOVERED_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static DRIVER_FAULT_REPLY_BUSY: AtomicBool = AtomicBool::new(false);
static TARGET_FAULT_ENDPOINT: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_CNODE: AtomicUsize = AtomicUsize::new(0);
static TARGET_ROOT_FAULT_TCB_CAP_SLOTS: [AtomicUsize; FAULT_REGISTRY_CAPACITY] =
    [const { AtomicUsize::new(0) }; FAULT_REGISTRY_CAPACITY];
static TARGET_ROOT_CONTROL_TEMPORAL_ACTIVE: AtomicBool = AtomicBool::new(false);
static TARGET_SERVICE_RECOVERY_SLOTS: [AtomicUsize; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicUsize::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_SERVICE_RECOVERY_STATES: [AtomicUsize; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicUsize::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];
static TARGET_SERVICE_CALL_SEQUENCES: [AtomicU64; SERVICE_FAULT_RECORD_CAPACITY] =
    [const { AtomicU64::new(0) }; SERVICE_FAULT_RECORD_CAPACITY];

const SERVICE_RECOVERY_UNREGISTERED: usize = 0;
const SERVICE_RECOVERY_READY: usize = 1;
const SERVICE_RECOVERY_REPLIED: usize = 2;

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
) -> Result<(), CriticalTcbConstructionError> {
    let mailbox = target_service_mailbox(task_id)?;
    match TARGET_SERVICE_RECOVERY_STATES[mailbox].load(Ordering::Acquire) {
        SERVICE_RECOVERY_READY => TARGET_SERVICE_CALL_SEQUENCES[mailbox]
            .compare_exchange(sequence, 0, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady),
        SERVICE_RECOVERY_REPLIED
            if TARGET_SERVICE_CALL_SEQUENCES[mailbox].load(Ordering::Acquire) == 0 =>
        {
            Ok(())
        }
        _ => Err(CriticalTcbConstructionError::RuntimeNotReady),
    }
}

/// Remove a contained service's recovery cap and prevent old-generation Reply
/// authority from surviving anchor revoke or reconstruction.
pub fn revoke_target_service_recovery_reply(
    task_id: &str,
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
    let error = sel4::cnode_delete(root_fault_cnode, slot, root_fault.cnode_radix_bits);
    if error != sel4_sys::seL4_NoError {
        return Err(sel4_error("critical.service-recovery-reply-delete", error));
    }
    TARGET_SERVICE_RECOVERY_SLOTS[mailbox].store(0, Ordering::Release);
    TARGET_SERVICE_RECOVERY_STATES[mailbox].store(SERVICE_RECOVERY_UNREGISTERED, Ordering::Release);
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
    TARGET_FAULT_REGISTRY.lock().seal()?;
    TARGET_FAULT_REGISTRY_SEALED.store(true, Ordering::Release);
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
    let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
        return PublishResult::Refused;
    };
    let result = handoff.publish_worker_control(record);
    drop(handoff);
    if result == PublishResult::Published {
        sel4::signal_unchecked(worker_signal_cap);
    }
    result
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

fn recover_target_passive_service_call(
    task_index: u16,
) -> Result<(), CriticalTcbConstructionError> {
    let mailbox = service_fault_mailbox_index(task_index)
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    TARGET_SERVICE_RECOVERY_STATES[mailbox]
        .compare_exchange(
            SERVICE_RECOVERY_READY,
            SERVICE_RECOVERY_REPLIED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|_| CriticalTcbConstructionError::RuntimeNotReady)?;
    let sequence = TARGET_SERVICE_CALL_SEQUENCES[mailbox].swap(0, Ordering::AcqRel);
    let reply_slot = TARGET_SERVICE_RECOVERY_SLOTS[mailbox].load(Ordering::Acquire) as seL4_CPtr;
    if reply_slot == sel4_sys::seL4_CapNull {
        return Err(CriticalTcbConstructionError::RuntimeNotReady);
    }
    // A passive child can fault before its first receive or between calls. In
    // that case there is no blocked donor to release, but the transition to
    // REPLIED still permanently closes this generation and prevents a later
    // request from acquiring the stale Reply object.
    if sequence == 0 {
        return Ok(());
    }
    sel4::set_message_register(0, sequence as seL4_Word);
    sel4::set_message_register(
        1,
        secure9p_transport::TransportError::Closed.wire_code() as seL4_Word,
    );
    sel4::reply_to(
        reply_slot,
        sel4_sys::seL4_MessageInfo::new(
            secure9p_transport::NAMESPACE_REJECTED_LABEL as seL4_Word,
            0,
            0,
            2,
        ),
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultReplyDisposition {
    Released,
    RetainedByDriver,
}

fn handle_target_fault(
    info: sel4_sys::seL4_MessageInfo,
    badge: seL4_Word,
) -> Result<FaultReplyDisposition, CriticalTcbConstructionError> {
    let (registration, fault_class) = resolve_target_fault(badge)?;
    let task = generated::temporal_tasks()
        .get(usize::from(registration.task_index))
        .ok_or(CriticalTcbConstructionError::MissingGeneratedRecord)?;
    let timeout_label = sel4_sys::SEL4_MCS_FAULT_TIMEOUT_LABEL as seL4_Word;
    if (fault_class == FaultClass::Timeout) != (info.label() == timeout_label) {
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
        tcb_cap: registration.tcb_cap,
    };
    let fault_handler_tcb_cap = root_fault_tcb_control_cap(registration.task_index)?;
    let disposition = match task.kind {
        TemporalTaskKind::Worker => {
            sel4::suspend_tcb(fault_handler_tcb_cap)
                .map_err(|error| sel4_error("critical.root-fault-worker-suspend", error))?;
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
        | TemporalTaskKind::DriverSupervisor => {
            sel4::suspend_tcb(fault_handler_tcb_cap)
                .map_err(|error| sel4_error("critical.root-fault-critical-suspend", error))?;
            sel4::signal_unchecked(CHILD_EMERGENCY_SIGNAL_SLOT);
            FaultReplyDisposition::Released
        }
        TemporalTaskKind::Service | TemporalTaskKind::Drain => {
            sel4::suspend_tcb(fault_handler_tcb_cap)
                .map_err(|error| sel4_error("critical.root-fault-service-suspend", error))?;
            if task.execution == generated::TemporalExecution::Passive
                && task.timeout_policy == TimeoutPolicy::ReturnError
            {
                recover_target_passive_service_call(registration.task_index)?;
            }
            publish_target_service_fault(record)?;
            FaultReplyDisposition::Released
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
        #[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
        cohesix_root_fault_qemu_evidence_turn();
        let mut badge = 0;
        let info = sel4::recv_with_reply(CHILD_INBOX_SLOT, &mut badge, CHILD_REPLY_SLOT);
        let disposition = match handle_target_fault(info, badge) {
            Ok(disposition) => disposition,
            Err(_) => target_fail_stop(
                "[critical] root-fault receive failed",
                Some(CHILD_EMERGENCY_SIGNAL_SLOT),
            ),
        };
        if disposition == FaultReplyDisposition::RetainedByDriver {
            let mut observed_badge = 0;
            let _ = sel4::wait(CHILD_DRIVER_RELEASE_SLOT, &mut observed_badge);
            if observed_badge != release_badge || DRIVER_FAULT_REPLY_BUSY.load(Ordering::Acquire) {
                target_fail_stop(
                    "[critical] root-fault driver release invalid",
                    Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                );
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
                handoff.drain_worker()
            };
            let Some(item) = item else {
                break;
            };
            let result = match item {
                WorkerSupervisorItem::Fault(record) => {
                    crate::worker_supervisor::drain_critical_fault(record)
                }
                WorkerSupervisorItem::Control(record) => {
                    crate::worker_supervisor::drain_critical_control(record)
                }
            };
            if result.is_err() {
                target_fail_stop(
                    "[critical] Worker supervisor containment failed",
                    Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                );
            }
        }
        let retry = match TARGET_HANDOFF.try_lock() {
            Some(handoff) => handoff.worker_pending(),
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

extern "C" fn root_driver_supervisor_entry(_arg0: seL4_Word) -> ! {
    let expected_badge = generated::worker_resource_admission_config()
        .handoff
        .driver_wake_badge;
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
            let record = {
                let Some(mut handoff) = TARGET_HANDOFF.try_lock() else {
                    // Publication precedes the wake, so a racing producer
                    // leaves a notification pending for the next bounded turn.
                    break;
                };
                handoff.drain_driver()
            };
            let Some(record) = record else {
                break;
            };
            if crate::hal::driver_task::root_driver_supervisor_contain_fault(record).is_err() {
                target_fail_stop(
                    "[critical] driver supervisor containment failed",
                    Some(CHILD_EMERGENCY_SIGNAL_SLOT),
                );
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
    core::hint::black_box(());
}

/// Stable external-QEMU observation point before root-emergency blocks.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_root_emergency_qemu_evidence_wait() {
    core::hint::black_box(());
}

/// Stable external-QEMU observation point before the Worker supervisor waits.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_worker_supervisor_qemu_evidence_wait() {
    core::hint::black_box(());
}

/// Stable external-QEMU observation point before the driver supervisor waits.
#[cfg(all(feature = "bootstrap-trace", feature = "release-qemu"))]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_driver_supervisor_qemu_evidence_wait() {
    core::hint::black_box(());
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
        WORKER_SUPERVISOR_ID | DRIVER_SUPERVISOR_ID => wake_notification,
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
        }
    }
    copy_child_cap(
        cnode,
        child_depth,
        CHILD_REPLY_SLOT,
        root_cnode,
        reply,
        root_depth,
        "critical.child-reply-slot",
    )?;

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
    env.bind_child_ipc_buffer(tcb, ipc_frame.cap(), ipc_frame.ptr().as_ptr() as usize)
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
    sel4::write_tcb_registers(tcb, entry, stack_top, 0, false)
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
    if timeout_fault_cap != sel4_sys::seL4_CapNull {
        sel4::set_tcb_timeout_endpoint(tcb, timeout_fault_cap)
            .map_err(|error| sel4_error("critical.tcb-timeout-endpoint", error))?;
    }
    Ok(())
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
    let handoff = generated::worker_resource_admission_config().handoff;
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
