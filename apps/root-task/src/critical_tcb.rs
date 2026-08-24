// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce generated critical-TCB reserves, bounded handoffs, and exact fault ownership.
// Author: Lukas Bower

//! Runtime model for the five independent Milestone 26e root duties.
//!
//! The queues below are wakeup companions, not authority.  Required fault
//! state lives in one durable per-slot mailbox and cannot be overwritten.  A
//! full policy queue refuses new work; a full fault mailbox is fatal because
//! silently dropping containment work would strand a TCB/SC/Reply relation.

use crate::generated::{self, TemporalExecution, TemporalTaskKind, TimeoutPolicy};
use crate::worker_supervisor::MAX_EXECUTABLE_WORKER_SLOTS;
use core::sync::atomic::{
    AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
};

/// Exact maximum generated root-control queue depth.
pub const WORKER_CONTROL_QUEUE_CAPACITY: usize = MAX_EXECUTABLE_WORKER_SLOTS;
const WORKER_CONTROL_RING_STORAGE: usize = WORKER_CONTROL_QUEUE_CAPACITY + 1;
/// Static storage ceiling for generated executable Worker fault owners.
pub const WORKER_FAULT_MAILBOX_CAPACITY: usize = MAX_EXECUTABLE_WORKER_SLOTS;
/// Storage ceiling for generated isolated-service fault owners.
pub const SERVICE_FAULT_RECORD_CAPACITY: usize = 2;
/// Exact maximum generated linked-driver fault records.
pub const DRIVER_FAULT_RECORD_CAPACITY: usize = 7;
/// Storage ceiling covering the larger Pi profile; QEMU seals its smaller
/// generated as-built registry without fabricating Pi-only TCBs.
pub const FAULT_REGISTRY_CAPACITY: usize = 80;
/// Number of independent critical root duties.
pub const CRITICAL_TCB_COUNT: usize = 5;

/// seL4 supplies two base replenishments; manifests record the total bound.
pub const MCS_BASE_REFILLS: u8 = 2;

const REQUIRED_CRITICAL_TCBS: [&str; CRITICAL_TCB_COUNT] = [
    "root-control",
    "root-fault",
    "root-emergency",
    "root-worker-supervisor",
    "root-driver-supervisor",
];

/// Immutable generation identity carried by a handoff record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GenerationIdentity {
    pub slot: u16,
    pub lease_epoch: u32,
    pub supervisor_generation: u32,
    pub cap_generation: u32,
}

/// Root-control operation admitted to the Worker supervisor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerControlOperation {
    Admit,
    Shutdown,
    Revoke,
}

/// One bounded root-control-to-Worker-supervisor record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerControlRecord {
    pub sequence: u64,
    /// Exact generated temporal-task index; Worker ABI slots are role-local.
    pub task_index: u16,
    pub identity: GenerationIdentity,
    pub operation: WorkerControlOperation,
}

impl WorkerControlOperation {
    const fn encode(self) -> u8 {
        match self {
            Self::Admit => 1,
            Self::Shutdown => 2,
            Self::Revoke => 3,
        }
    }

    const fn decode(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Admit),
            2 => Some(Self::Shutdown),
            3 => Some(Self::Revoke),
            _ => None,
        }
    }
}

struct AtomicWorkerControlRecord {
    sequence: AtomicU64,
    task_index: AtomicU16,
    identity_slot: AtomicU16,
    lease_epoch: AtomicU32,
    supervisor_generation: AtomicU32,
    cap_generation: AtomicU32,
    operation: AtomicU8,
}

impl AtomicWorkerControlRecord {
    const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            task_index: AtomicU16::new(0),
            identity_slot: AtomicU16::new(0),
            lease_epoch: AtomicU32::new(0),
            supervisor_generation: AtomicU32::new(0),
            cap_generation: AtomicU32::new(0),
            operation: AtomicU8::new(0),
        }
    }

    fn store(&self, record: WorkerControlRecord) {
        self.sequence.store(record.sequence, Ordering::Relaxed);
        self.task_index.store(record.task_index, Ordering::Relaxed);
        self.identity_slot
            .store(record.identity.slot, Ordering::Relaxed);
        self.lease_epoch
            .store(record.identity.lease_epoch, Ordering::Relaxed);
        self.supervisor_generation
            .store(record.identity.supervisor_generation, Ordering::Relaxed);
        self.cap_generation
            .store(record.identity.cap_generation, Ordering::Relaxed);
        self.operation
            .store(record.operation.encode(), Ordering::Relaxed);
    }

    fn load(&self) -> Result<WorkerControlRecord, WorkerControlQueueError> {
        let operation = WorkerControlOperation::decode(self.operation.load(Ordering::Relaxed))
            .ok_or(WorkerControlQueueError::InvalidRecord)?;
        Ok(WorkerControlRecord {
            sequence: self.sequence.load(Ordering::Relaxed),
            task_index: self.task_index.load(Ordering::Relaxed),
            identity: GenerationIdentity {
                slot: self.identity_slot.load(Ordering::Relaxed),
                lease_epoch: self.lease_epoch.load(Ordering::Relaxed),
                supervisor_generation: self.supervisor_generation.load(Ordering::Relaxed),
                cap_generation: self.cap_generation.load(Ordering::Relaxed),
            },
            operation,
        })
    }
}

/// Internal corruption or ownership violation in the atomic control ring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerControlQueueError {
    /// A ring cell did not contain a valid compiler-declared operation.
    InvalidRecord,
    /// More than one critical validator attempted to advance the pipeline.
    ValidatorContended,
    /// More than one root consumer attempted to drain validated policy.
    ConsumerContended,
}

/// Bounded wait-free root-control to Worker-supervisor policy pipeline.
///
/// Root-control publishes, the restricted Worker-supervisor validates, and
/// root-control consumes the validated record. Three monotonic ring cursors
/// preserve that ordering without copying records into a second queue or
/// sharing a cross-core lock. Capacity is released only after final consume.
pub struct WorkerControlQueue {
    cells: [AtomicWorkerControlRecord; WORKER_CONTROL_RING_STORAGE],
    head: AtomicUsize,
    validated: AtomicUsize,
    tail: AtomicUsize,
    producer_active: AtomicBool,
    validator_active: AtomicBool,
    consumer_active: AtomicBool,
}

impl Default for WorkerControlQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerControlQueue {
    /// Create an empty bounded ring.
    pub const fn new() -> Self {
        Self {
            cells: [const { AtomicWorkerControlRecord::new() }; WORKER_CONTROL_RING_STORAGE],
            head: AtomicUsize::new(0),
            validated: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            producer_active: AtomicBool::new(false),
            validator_active: AtomicBool::new(false),
            consumer_active: AtomicBool::new(false),
        }
    }

    const fn advance(index: usize) -> usize {
        if index + 1 == WORKER_CONTROL_RING_STORAGE {
            0
        } else {
            index + 1
        }
    }

    fn len_from(head: usize, tail: usize) -> usize {
        if tail >= head {
            tail - head
        } else {
            WORKER_CONTROL_RING_STORAGE - head + tail
        }
    }

    /// Publish one exact record without sharing the fault-mailbox lock.
    pub fn publish(&self, record: WorkerControlRecord) -> PublishResult {
        if worker_fault_mailbox_index(record.task_index).is_none()
            || self
                .producer_active
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
        {
            return PublishResult::Refused;
        }
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let generated_capacity = usize::from(
            generated::worker_resource_admission_config()
                .handoff
                .worker_control_queue_capacity,
        );
        if Self::len_from(head, tail) >= generated_capacity {
            self.producer_active.store(false, Ordering::Release);
            return PublishResult::Refused;
        }
        self.cells[tail].store(record);
        self.tail.store(Self::advance(tail), Ordering::Release);
        self.producer_active.store(false, Ordering::Release);
        PublishResult::Published
    }

    /// Validate one published FIFO record on the sole restricted child.
    pub fn validate_next(&self) -> Result<Option<WorkerControlRecord>, WorkerControlQueueError> {
        if self
            .validator_active
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(WorkerControlQueueError::ValidatorContended);
        }
        let validated = self.validated.load(Ordering::Relaxed);
        if validated == self.tail.load(Ordering::Acquire) {
            self.validator_active.store(false, Ordering::Release);
            return Ok(None);
        }
        let record = match self.cells[validated].load() {
            Ok(record)
                if record.sequence != 0
                    && worker_fault_mailbox_index(record.task_index).is_some()
                    && record.identity.supervisor_generation != 0
                    && record.identity.cap_generation != 0 =>
            {
                record
            }
            _ => {
                self.validator_active.store(false, Ordering::Release);
                return Err(WorkerControlQueueError::InvalidRecord);
            }
        };
        self.validated
            .store(Self::advance(validated), Ordering::Release);
        self.validator_active.store(false, Ordering::Release);
        Ok(Some(record))
    }

    /// Drain one critically validated FIFO record on the sole root consumer.
    pub fn drain_validated(&self) -> Result<Option<WorkerControlRecord>, WorkerControlQueueError> {
        if self
            .consumer_active
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(WorkerControlQueueError::ConsumerContended);
        }
        let head = self.head.load(Ordering::Relaxed);
        if head == self.validated.load(Ordering::Acquire) {
            self.consumer_active.store(false, Ordering::Release);
            return Ok(None);
        }
        let record = self.cells[head].load();
        if record.is_ok() {
            self.head.store(Self::advance(head), Ordering::Release);
        }
        self.consumer_active.store(false, Ordering::Release);
        record.map(Some)
    }

    /// Number of published records not yet consumed.
    #[must_use]
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        Self::len_from(head, tail)
    }

    /// Number of published records awaiting critical validation.
    #[must_use]
    pub fn unvalidated_len(&self) -> usize {
        let validated = self.validated.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        Self::len_from(validated, tail)
    }

    /// Number of critically validated records awaiting root consumption.
    #[must_use]
    pub fn validated_len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let validated = self.validated.load(Ordering::Acquire);
        Self::len_from(head, validated)
    }

    /// Whether the ring contains no published record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether one published record remains.
    #[must_use]
    pub fn pending(&self) -> bool {
        !self.is_empty()
    }

    /// Whether the restricted child has published policy left to validate.
    #[must_use]
    pub fn validation_pending(&self) -> bool {
        self.unvalidated_len() != 0
    }

    /// Whether root-control has critically validated policy left to consume.
    #[must_use]
    pub fn validated_pending(&self) -> bool {
        self.validated_len() != 0
    }
}

/// Fault class routed to an independent supervisor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultClass {
    Standard,
    Timeout,
}

/// Durable fault/containment record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultHandoffRecord {
    pub sequence: u64,
    pub task_index: u16,
    pub identity: GenerationIdentity,
    pub fault_badge: u64,
    pub fault_class: FaultClass,
    /// Raw seL4 fault label copied before root-fault yields.
    pub fault_label: u64,
    /// Exact message length supplied by the kernel.
    pub fault_length: u16,
    /// First two raw message registers. Timeout faults encode Data/Consumed
    /// here; standard faults retain their class-specific leading operands.
    pub fault_mr0: u64,
    pub fault_mr1: u64,
    pub tcb_cap: usize,
}

/// Result of a non-blocking critical producer handoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishResult {
    Published,
    Refused,
}

/// Fatal fault-handoff construction or saturation error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultHandoffError {
    SlotOutOfRange,
    MailboxOccupied,
    Contended,
    RegistryUnknown,
}

/// One item drained by the Worker supervisor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerSupervisorItem {
    Fault(FaultHandoffRecord),
    Control(WorkerControlRecord),
}

/// Map a generated temporal task index to its distinct Worker mailbox.
///
/// Worker ABI slots are role-local, so Heartbeat, GPU, and LoRA may all carry
/// `identity.slot == 0`. The exact temporal-task ordinal is the non-aliasing
/// registry key for their supervisor mailboxes.
#[must_use]
pub fn worker_fault_mailbox_index(task_index: u16) -> Option<usize> {
    let task_index = usize::from(task_index);
    let tasks = generated::temporal_tasks();
    let task = tasks.get(task_index)?;
    if task.kind != TemporalTaskKind::Worker {
        return None;
    }
    let mailbox = tasks[..task_index]
        .iter()
        .filter(|task| task.kind == TemporalTaskKind::Worker)
        .count();
    let generated_capacity = usize::from(
        generated::worker_resource_admission_config()
            .fault_registry
            .worker_tcbs,
    );
    (mailbox < generated_capacity && mailbox < WORKER_FAULT_MAILBOX_CAPACITY).then_some(mailbox)
}

/// Map a generated temporal task index to its distinct service-owner mailbox.
///
/// Both active services and passive donated-Call services terminate through
/// their root-control owner; a generated Drain uses this same bounded lane.
#[must_use]
pub fn service_fault_mailbox_index(task_index: u16) -> Option<usize> {
    let task_index = usize::from(task_index);
    let tasks = generated::temporal_tasks();
    let task = tasks.get(task_index)?;
    if !matches!(
        task.kind,
        TemporalTaskKind::Service | TemporalTaskKind::Drain
    ) {
        return None;
    }
    let mailbox = tasks[..task_index]
        .iter()
        .filter(|task| {
            matches!(
                task.kind,
                TemporalTaskKind::Service | TemporalTaskKind::Drain
            )
        })
        .count();
    let generated_capacity = usize::from(
        generated::worker_resource_admission_config()
            .fault_registry
            .service_tcbs,
    );
    (mailbox < generated_capacity && mailbox < SERVICE_FAULT_RECORD_CAPACITY).then_some(mailbox)
}

/// Compiler-validated authority retained by root-fault for one passive
/// service's donated Call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassiveServiceRecoveryContract {
    pub task_index: u16,
    pub mailbox: usize,
    pub root_fault_reply_slot: u32,
}

/// Why a service is not eligible for the passive donated-Call recovery path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassiveServiceRecoveryContractError {
    MissingTask,
    ActiveOrUnownedService,
    InvalidReplySlot,
}

/// Resolve the sole generated passive service recovery contract.
///
/// This deliberately rejects active services such as console-network: their
/// independently scheduled receive loops never share NineDoor's retained
/// Reply authority or typed donor-release path.
pub fn passive_service_recovery_contract(
    task_id: &str,
) -> Result<PassiveServiceRecoveryContract, PassiveServiceRecoveryContractError> {
    let task_index = generated::temporal_tasks()
        .iter()
        .position(|task| task.id == task_id)
        .ok_or(PassiveServiceRecoveryContractError::MissingTask)?;
    let task = &generated::temporal_tasks()[task_index];
    let config = generated::ninedoor_service_config();
    if task_id != "ninedoor-service"
        || !config.enabled
        || task.kind != TemporalTaskKind::Service
        || task.execution != TemporalExecution::Passive
        || task.timeout_policy != TimeoutPolicy::ReturnError
        || task.allowed_donors != ["root-control"]
        || task.reply_objects != 1
        || task.max_donation_depth != 1
        || task.scheduling_context_slot != 0
        || task.scheduling_context_bits != 0
    {
        return Err(PassiveServiceRecoveryContractError::ActiveOrUnownedService);
    }
    let root_fault = generated::worker_resource_admission_config()
        .critical_tcbs
        .iter()
        .find(|resource| resource.id == "root-fault")
        .ok_or(PassiveServiceRecoveryContractError::InvalidReplySlot)?;
    if config.root_fault_recovery_reply_slot == 0
        || config.root_fault_recovery_reply_slot >= (1u32 << root_fault.cnode_radix_bits)
    {
        return Err(PassiveServiceRecoveryContractError::InvalidReplySlot);
    }
    let task_index =
        u16::try_from(task_index).map_err(|_| PassiveServiceRecoveryContractError::MissingTask)?;
    let mailbox = service_fault_mailbox_index(task_index)
        .ok_or(PassiveServiceRecoveryContractError::MissingTask)?;
    Ok(PassiveServiceRecoveryContract {
        task_index,
        mailbox,
        root_fault_reply_slot: config.root_fault_recovery_reply_slot,
    })
}

/// Validate one coalesced Worker-supervisor wake against generated one-hot bits.
///
/// The return value says whether the root critical-handoff bit was present;
/// child READY/completion bits may appear alone or in any combination.
#[must_use]
pub fn validate_worker_supervisor_wake(badge: u64) -> Option<bool> {
    let handoff = generated::worker_resource_admission_config()
        .handoff
        .worker_wake_badge;
    let abi = generated::worker_runtime_config().task_abi;
    let bits = [
        handoff,
        abi.heartbeat_wake_bit,
        abi.gpu_wake_bit,
        abi.lora_wake_bit,
    ];
    if bits.iter().any(|bit| !bit.is_power_of_two()) {
        return None;
    }
    for left in 0..bits.len() {
        for right in left + 1..bits.len() {
            if bits[left] == bits[right] {
                return None;
            }
        }
    }
    let allowed = bits.into_iter().fold(0, |mask, bit| mask | bit);
    if badge == 0 || badge & !allowed != 0 {
        None
    } else {
        Some(badge & handoff != 0)
    }
}

/// Bounded non-blocking handoff state shared by critical root TCBs.
pub struct CriticalHandoff {
    worker_faults: [Option<FaultHandoffRecord>; WORKER_FAULT_MAILBOX_CAPACITY],
    service_faults: [Option<FaultHandoffRecord>; SERVICE_FAULT_RECORD_CAPACITY],
    driver_faults: [Option<FaultHandoffRecord>; DRIVER_FAULT_RECORD_CAPACITY],
    fatal_fault_handoff: bool,
}

impl Default for CriticalHandoff {
    fn default() -> Self {
        Self::new()
    }
}

impl CriticalHandoff {
    /// Create empty bounded handoff state suitable for static target storage.
    pub const fn new() -> Self {
        Self {
            worker_faults: [None; WORKER_FAULT_MAILBOX_CAPACITY],
            service_faults: [None; SERVICE_FAULT_RECORD_CAPACITY],
            driver_faults: [None; DRIVER_FAULT_RECORD_CAPACITY],
            fatal_fault_handoff: false,
        }
    }

    /// Validate compile-time capacities and generated drain precedence.
    pub fn validate_generated_contract() -> Result<(), CriticalTopologyError> {
        let config = generated::worker_resource_admission_config();
        if !config.enabled
            || config.handoff.worker_control_queue_capacity == 0
            || usize::from(config.handoff.worker_control_queue_capacity)
                > WORKER_CONTROL_QUEUE_CAPACITY
            || config.handoff.worker_fault_mailboxes == 0
            || usize::from(config.handoff.worker_fault_mailboxes) > WORKER_FAULT_MAILBOX_CAPACITY
            || usize::from(config.fault_registry.service_tcbs) > SERVICE_FAULT_RECORD_CAPACITY
            || config.handoff.service_fault_badges.count != config.fault_registry.service_tcbs
            || usize::from(config.handoff.driver_fault_records) > DRIVER_FAULT_RECORD_CAPACITY
            || config.handoff.worker_drain_precedence
                != [
                    generated::HandoffClass::WorkerFault,
                    generated::HandoffClass::WorkerControl,
                ]
            || config.handoff.driver_drain_precedence != [generated::HandoffClass::DriverFault]
        {
            return Err(CriticalTopologyError::GeneratedCapacityMismatch);
        }
        Ok(())
    }

    /// Publish one required Worker fault record into its exact slot mailbox.
    pub fn publish_worker_fault(
        &mut self,
        record: FaultHandoffRecord,
    ) -> Result<(), FaultHandoffError> {
        let Some(slot) = worker_fault_mailbox_index(record.task_index) else {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::SlotOutOfRange);
        };
        let Some(mailbox) = self.worker_faults.get_mut(slot) else {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::SlotOutOfRange);
        };
        if mailbox.is_some() {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::MailboxOccupied);
        }
        *mailbox = Some(record);
        Ok(())
    }

    /// Publish one required isolated-service fault to its exact owner mailbox.
    pub fn publish_service_fault(
        &mut self,
        record: FaultHandoffRecord,
    ) -> Result<(), FaultHandoffError> {
        let Some(slot) = service_fault_mailbox_index(record.task_index) else {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::SlotOutOfRange);
        };
        let Some(mailbox) = self.service_faults.get_mut(slot) else {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::SlotOutOfRange);
        };
        if mailbox.is_some() {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::MailboxOccupied);
        }
        *mailbox = Some(record);
        Ok(())
    }

    /// Publish one required driver fault record into its exact runtime mailbox.
    pub fn publish_driver_fault(
        &mut self,
        runtime_slot: u16,
        record: FaultHandoffRecord,
    ) -> Result<(), FaultHandoffError> {
        if runtime_slot
            >= generated::worker_resource_admission_config()
                .handoff
                .driver_fault_records
        {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::SlotOutOfRange);
        }
        let Some(mailbox) = self.driver_faults.get_mut(usize::from(runtime_slot)) else {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::SlotOutOfRange);
        };
        if mailbox.is_some() {
            self.fatal_fault_handoff = true;
            return Err(FaultHandoffError::MailboxOccupied);
        }
        *mailbox = Some(record);
        Ok(())
    }

    /// Drain one Worker fault before the caller considers policy work.
    pub fn drain_worker_fault(&mut self) -> Option<FaultHandoffRecord> {
        for mailbox in &mut self.worker_faults {
            if let Some(record) = mailbox.take() {
                return Some(record);
            }
        }
        None
    }

    /// Drain one exact isolated-service fault for the generated owner.
    pub fn drain_service(
        &mut self,
        task_index: u16,
    ) -> Result<Option<FaultHandoffRecord>, FaultHandoffError> {
        let slot =
            service_fault_mailbox_index(task_index).ok_or(FaultHandoffError::SlotOutOfRange)?;
        let mailbox = self
            .service_faults
            .get_mut(slot)
            .ok_or(FaultHandoffError::SlotOutOfRange)?;
        Ok(mailbox.take())
    }

    /// Drain one exact driver containment record.
    pub fn drain_driver(&mut self) -> Option<FaultHandoffRecord> {
        for mailbox in &mut self.driver_faults {
            if let Some(record) = mailbox.take() {
                return Some(record);
            }
        }
        None
    }

    /// Whether a required fault handoff attempted to overwrite or exceed its pool.
    #[must_use]
    pub const fn fatal_fault_handoff(&self) -> bool {
        self.fatal_fault_handoff
    }

    /// Whether any Worker fault record remains after a drain turn.
    #[must_use]
    pub fn worker_fault_pending(&self) -> bool {
        self.worker_faults.iter().any(Option::is_some)
    }

    /// Whether any driver containment record remains after a drain turn.
    #[must_use]
    pub fn driver_pending(&self) -> bool {
        self.driver_faults.iter().any(Option::is_some)
    }

    /// Whether a suspended service awaits root-control containment.
    #[must_use]
    pub fn service_pending(&self) -> bool {
        self.service_faults.iter().any(Option::is_some)
    }
}

/// Exact source registered for one standard/timeout badge pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultRegistration {
    pub task_index: u16,
    pub identity: GenerationIdentity,
    pub standard_badge: u64,
    pub timeout_badge: u64,
    pub tcb_cap: usize,
    pub terminal: bool,
}

/// Fatal registry error; callers must stop construction rather than ignore it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultRegistryError {
    InvalidRegistration,
    DuplicateTask,
    DuplicateTcb,
    DuplicateBadge,
    Overflow,
    Incomplete,
    UnknownBadge,
    UnknownTask,
    IdentityMismatch,
    GenerationNotNewer,
    Sealed,
    NotSealed,
}

/// Fixed registry sized from every admitted temporal TCB.
pub struct FaultRegistry {
    entries: [Option<FaultRegistration>; FAULT_REGISTRY_CAPACITY],
    len: usize,
    sealed: bool,
}

/// Lock-independent copy of the bounded live fault registry for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultRegistrySnapshot {
    pub entries: [Option<FaultRegistration>; FAULT_REGISTRY_CAPACITY],
    pub len: usize,
    pub sealed: bool,
}

impl FaultRegistrySnapshot {
    #[must_use]
    pub fn registration(&self, task_index: u16) -> Option<FaultRegistration> {
        self.entries[..self.len]
            .iter()
            .flatten()
            .find(|entry| entry.task_index == task_index)
            .copied()
    }
}

impl Default for FaultRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FaultRegistry {
    /// Create an empty exact registry suitable for static target storage.
    pub const fn new() -> Self {
        Self {
            entries: [None; FAULT_REGISTRY_CAPACITY],
            len: 0,
            sealed: false,
        }
    }

    /// Validate the registry bound against generated truth.
    pub fn validate_generated_capacity() -> Result<(), FaultRegistryError> {
        let config = generated::worker_resource_admission_config();
        let expected = usize::from(config.fault_registry.capacity);
        if expected == 0
            || expected > FAULT_REGISTRY_CAPACITY
            || generated::temporal_tasks().len() != expected
        {
            return Err(FaultRegistryError::Overflow);
        }
        Ok(())
    }

    /// Register one exact TCB identity, rejecting all duplicate/overflow cases.
    pub fn register(&mut self, registration: FaultRegistration) -> Result<(), FaultRegistryError> {
        if self.sealed {
            return Err(FaultRegistryError::Sealed);
        }
        if self.len
            >= usize::from(
                generated::worker_resource_admission_config()
                    .fault_registry
                    .capacity,
            )
        {
            return Err(FaultRegistryError::Overflow);
        }
        if registration.standard_badge == 0
            || registration.timeout_badge == 0
            || registration.standard_badge == registration.timeout_badge
            || usize::from(registration.task_index) >= generated::temporal_tasks().len()
            || registration.tcb_cap == 0
            || registration.identity.lease_epoch == 0
            || registration.identity.supervisor_generation == 0
            || registration.identity.cap_generation == 0
        {
            return Err(FaultRegistryError::InvalidRegistration);
        }
        if self.entries[..self.len]
            .iter()
            .flatten()
            .any(|entry| entry.task_index == registration.task_index)
        {
            return Err(FaultRegistryError::DuplicateTask);
        }
        if self.entries[..self.len]
            .iter()
            .flatten()
            .any(|entry| entry.tcb_cap == registration.tcb_cap)
        {
            return Err(FaultRegistryError::DuplicateTcb);
        }
        if self.entries[..self.len].iter().flatten().any(|entry| {
            entry.standard_badge == registration.standard_badge
                || entry.timeout_badge == registration.timeout_badge
                || entry.standard_badge == registration.timeout_badge
                || entry.timeout_badge == registration.standard_badge
        }) {
            return Err(FaultRegistryError::DuplicateBadge);
        }
        let task = &generated::temporal_tasks()[usize::from(registration.task_index)];
        if generated_standard_fault_badge(task.id) != Some(registration.standard_badge)
            || task.timeout_badge != registration.timeout_badge
            || registration.terminal
                != (task.timeout_policy != generated::TimeoutPolicy::ReplenishOnce)
        {
            return Err(FaultRegistryError::InvalidRegistration);
        }
        let Some(entry) = self.entries.get_mut(self.len) else {
            return Err(FaultRegistryError::Overflow);
        };
        *entry = Some(registration);
        self.len += 1;
        Ok(())
    }

    /// Replace one sealed source after its prior TCB generation was contained.
    ///
    /// The generated task index, badge pair, terminal policy, and registry
    /// cardinality are immutable. Only the live TCB cap and generation identity
    /// advance, and both supervisor-owned generation counters must increase.
    pub fn replace(
        &mut self,
        prior_identity: GenerationIdentity,
        replacement: FaultRegistration,
    ) -> Result<(), FaultRegistryError> {
        if !self.sealed {
            return Err(FaultRegistryError::NotSealed);
        }
        let Some(entry_index) = self.entries[..self.len].iter().position(|entry| {
            entry.is_some_and(|entry| entry.task_index == replacement.task_index)
        }) else {
            return Err(FaultRegistryError::UnknownTask);
        };
        let prior = self.entries[entry_index].ok_or(FaultRegistryError::UnknownTask)?;
        if prior.identity != prior_identity {
            return Err(FaultRegistryError::IdentityMismatch);
        }
        if replacement.standard_badge != prior.standard_badge
            || replacement.timeout_badge != prior.timeout_badge
            || replacement.terminal != prior.terminal
        {
            return Err(FaultRegistryError::InvalidRegistration);
        }
        if replacement.tcb_cap == 0
            || replacement.identity.slot != prior_identity.slot
            || replacement.identity.lease_epoch == 0
            || replacement.identity.supervisor_generation == 0
            || replacement.identity.cap_generation == 0
        {
            return Err(FaultRegistryError::InvalidRegistration);
        }
        if replacement.identity.supervisor_generation <= prior_identity.supervisor_generation
            || replacement.identity.cap_generation <= prior_identity.cap_generation
        {
            return Err(FaultRegistryError::GenerationNotNewer);
        }
        if self.entries[..self.len]
            .iter()
            .enumerate()
            .any(|(index, entry)| {
                index != entry_index
                    && entry.is_some_and(|entry| entry.tcb_cap == replacement.tcb_cap)
            })
        {
            return Err(FaultRegistryError::DuplicateTcb);
        }
        self.entries[entry_index] = Some(replacement);
        Ok(())
    }

    /// Seal an exact complete registry without consuming static storage.
    pub fn seal(&mut self) -> Result<(), FaultRegistryError> {
        if self.sealed {
            return Err(FaultRegistryError::Sealed);
        }
        self.validate_complete()?;
        self.sealed = true;
        Ok(())
    }

    /// Seal construction only when every compiler-admitted TCB is registered.
    pub fn finish(mut self) -> Result<Self, FaultRegistryError> {
        self.seal()?;
        Ok(self)
    }

    /// Validate exact completeness without consuming a statically held registry.
    pub fn validate_complete(&self) -> Result<(), FaultRegistryError> {
        if self.len
            == usize::from(
                generated::worker_resource_admission_config()
                    .fault_registry
                    .capacity,
            )
        {
            Ok(())
        } else {
            Err(FaultRegistryError::Incomplete)
        }
    }

    /// Resolve a standard or timeout badge without inference.
    pub fn resolve(
        &self,
        badge: u64,
    ) -> Result<(FaultRegistration, FaultClass), FaultRegistryError> {
        for entry in self.entries[..self.len].iter().flatten() {
            if entry.standard_badge == badge {
                return Ok((*entry, FaultClass::Standard));
            }
            if entry.timeout_badge == badge {
                return Ok((*entry, FaultClass::Timeout));
            }
        }
        Err(FaultRegistryError::UnknownBadge)
    }

    /// Number of exact live TCB registrations.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the registry is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether one generated temporal-task index has an exact live entry.
    #[must_use]
    pub fn contains_task_index(&self, task_index: u16) -> bool {
        self.entries[..self.len]
            .iter()
            .flatten()
            .any(|entry| entry.task_index == task_index)
    }

    #[must_use]
    pub const fn snapshot(&self) -> FaultRegistrySnapshot {
        FaultRegistrySnapshot {
            entries: self.entries,
            len: self.len,
            sealed: self.sealed,
        }
    }
}

/// Return the compiler-owned standard-fault badge for one exact temporal task.
///
/// Category-local ordering follows the generated temporal table. This keeps
/// critical, service, Worker, and driver identities disjoint without allowing
/// constructors to invent badges.
#[must_use]
pub fn generated_standard_fault_badge(task_id: &str) -> Option<u64> {
    let tasks = generated::temporal_tasks();
    let task = tasks.iter().find(|task| task.id == task_id)?;
    let handoff = generated::worker_resource_admission_config().handoff;
    let (range, category_index) = match task.kind {
        TemporalTaskKind::RootControl
        | TemporalTaskKind::RootFault
        | TemporalTaskKind::RootEmergency
        | TemporalTaskKind::WorkerSupervisor
        | TemporalTaskKind::DriverSupervisor => (
            handoff.critical_fault_badges,
            tasks
                .iter()
                .filter(|candidate| {
                    matches!(
                        candidate.kind,
                        TemporalTaskKind::RootControl
                            | TemporalTaskKind::RootFault
                            | TemporalTaskKind::RootEmergency
                            | TemporalTaskKind::WorkerSupervisor
                            | TemporalTaskKind::DriverSupervisor
                    )
                })
                .position(|candidate| candidate.id == task_id)?,
        ),
        TemporalTaskKind::Worker => (
            handoff.worker_fault_badges,
            tasks
                .iter()
                .filter(|candidate| candidate.kind == TemporalTaskKind::Worker)
                .position(|candidate| candidate.id == task_id)?,
        ),
        TemporalTaskKind::Driver => (
            handoff.driver_fault_badges,
            tasks
                .iter()
                .filter(|candidate| candidate.kind == TemporalTaskKind::Driver)
                .position(|candidate| candidate.id == task_id)?,
        ),
        TemporalTaskKind::Service | TemporalTaskKind::Drain => (
            handoff.service_fault_badges,
            tasks
                .iter()
                .filter(|candidate| {
                    matches!(
                        candidate.kind,
                        TemporalTaskKind::Service | TemporalTaskKind::Drain
                    )
                })
                .position(|candidate| candidate.id == task_id)?,
        ),
    };
    if category_index >= usize::from(range.count) {
        return None;
    }
    range.base.checked_add(
        u64::try_from(category_index)
            .ok()?
            .checked_mul(u64::from(range.stride))?,
    )
}

/// Ownership state for one root-fault MCS receive/Reply lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultReplyLaneState {
    Free,
    Associated {
        registration: FaultRegistration,
        class: FaultClass,
        replied: bool,
    },
}

/// Invalid Reply-lane transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultReplyLaneError {
    Busy,
    Free,
    ReplyForbidden,
    ReplyAlreadyIssued,
    AssociationNotCleared,
}

/// Single-owner Reply lane used by root-fault.
pub struct FaultReplyLane {
    state: FaultReplyLaneState,
}

impl Default for FaultReplyLane {
    fn default() -> Self {
        Self {
            state: FaultReplyLaneState::Free,
        }
    }
}

impl FaultReplyLane {
    /// Associate the lane with exactly one received fault.
    pub fn begin(
        &mut self,
        registration: FaultRegistration,
        class: FaultClass,
    ) -> Result<(), FaultReplyLaneError> {
        if self.state != FaultReplyLaneState::Free {
            return Err(FaultReplyLaneError::Busy);
        }
        self.state = FaultReplyLaneState::Associated {
            registration,
            class,
            replied: false,
        };
        Ok(())
    }

    /// Finish a terminal fault only after Suspend cleared the association.
    pub fn finish_terminal(
        &mut self,
        tcb_suspended: bool,
        association_clear: bool,
    ) -> Result<(), FaultReplyLaneError> {
        let FaultReplyLaneState::Associated { replied, .. } = self.state else {
            return Err(FaultReplyLaneError::Free);
        };
        if replied || !tcb_suspended || !association_clear {
            return Err(FaultReplyLaneError::AssociationNotCleared);
        }
        self.state = FaultReplyLaneState::Free;
        Ok(())
    }

    /// Issue the sole typed Reply for a compiler-allowlisted recoverable timeout.
    pub fn reply_recoverable_timeout(
        &mut self,
        allowlisted: bool,
    ) -> Result<(), FaultReplyLaneError> {
        let FaultReplyLaneState::Associated {
            class,
            ref mut replied,
            ..
        } = self.state
        else {
            return Err(FaultReplyLaneError::Free);
        };
        if class != FaultClass::Timeout || !allowlisted {
            return Err(FaultReplyLaneError::ReplyForbidden);
        }
        if *replied {
            return Err(FaultReplyLaneError::ReplyAlreadyIssued);
        }
        *replied = true;
        Ok(())
    }

    /// Release a recoverable lane only after its one Reply cleared association.
    pub fn finish_recoverable(
        &mut self,
        association_clear: bool,
    ) -> Result<(), FaultReplyLaneError> {
        let FaultReplyLaneState::Associated {
            class: FaultClass::Timeout,
            replied: true,
            ..
        } = self.state
        else {
            return Err(FaultReplyLaneError::ReplyForbidden);
        };
        if !association_clear {
            return Err(FaultReplyLaneError::AssociationNotCleared);
        }
        self.state = FaultReplyLaneState::Free;
        Ok(())
    }

    #[must_use]
    pub const fn state(&self) -> FaultReplyLaneState {
        self.state
    }
}

/// Constructed kernel-object handles for one critical duty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CriticalTcbHandle {
    pub id: &'static str,
    pub origin: CriticalTcbOrigin,
    pub tcb_cap: usize,
    pub cnode_cap: usize,
    pub sched_context_cap: usize,
    pub sched_control_cap: usize,
    pub fault_endpoint_cap: usize,
    pub timeout_endpoint_cap: usize,
    pub reply_cap: usize,
    pub wake_notification_cap: usize,
    /// Manifest-named slot retaining this permanent domain's CNode cap.
    ///
    /// This is not a grouped child-untyped reclamation anchor.
    pub revoke_anchor_cap: usize,
    pub core: u8,
}

/// Construction origin for a critical root progress domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriticalTcbOrigin {
    /// The real root-control event loop remains on the init TCB and initial SC.
    InitRootControl,
    /// A separately allocated TCB with a restricted child CSpace and active SC.
    RestrictedChild,
}

/// Fatal critical topology construction failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriticalTopologyError {
    GeneratedCapacityMismatch,
    MissingGeneratedTask,
    DuplicateDuty,
    DuplicateKernelObject,
    MissingKernelObject,
    TemporalMismatch,
    Incomplete,
}

/// Convert the compiler's total refill bound to seL4's extra-refill argument.
pub const fn mcs_extra_refills(max_refills: u8) -> Result<u8, CriticalTopologyError> {
    match max_refills.checked_sub(MCS_BASE_REFILLS) {
        Some(extra) => Ok(extra),
        None => Err(CriticalTopologyError::TemporalMismatch),
    }
}

/// Exact five-duty constructed inventory.
pub struct CriticalTcbInventory {
    handles: [Option<CriticalTcbHandle>; CRITICAL_TCB_COUNT],
    len: usize,
}

impl Default for CriticalTcbInventory {
    fn default() -> Self {
        Self {
            handles: [None; CRITICAL_TCB_COUNT],
            len: 0,
        }
    }
}

impl CriticalTcbInventory {
    /// Add one fully constructed duty. No error may be ignored by callers.
    pub fn register(&mut self, handle: CriticalTcbHandle) -> Result<(), CriticalTopologyError> {
        let task = generated::temporal_tasks()
            .iter()
            .find(|task| task.id == handle.id)
            .ok_or(CriticalTopologyError::MissingGeneratedTask)?;
        if !REQUIRED_CRITICAL_TCBS.contains(&handle.id)
            || task.execution != TemporalExecution::Active
            || !task.critical_reserve
            || task.core != handle.core
            || task.scheduling_context_slot == 0
        {
            return Err(CriticalTopologyError::TemporalMismatch);
        }
        let expected_origin = if handle.id == "root-control" {
            CriticalTcbOrigin::InitRootControl
        } else {
            CriticalTcbOrigin::RestrictedChild
        };
        if handle.origin != expected_origin {
            return Err(CriticalTopologyError::TemporalMismatch);
        }
        let required_objects = [
            handle.tcb_cap,
            handle.cnode_cap,
            handle.sched_context_cap,
            handle.sched_control_cap,
            handle.reply_cap,
            handle.wake_notification_cap,
            handle.revoke_anchor_cap,
        ];
        if required_objects.contains(&0)
            || (handle.id != "root-emergency"
                && [handle.fault_endpoint_cap, handle.timeout_endpoint_cap].contains(&0))
        {
            return Err(CriticalTopologyError::MissingKernelObject);
        }
        if self.handles[..self.len]
            .iter()
            .flatten()
            .any(|prior| prior.id == handle.id)
        {
            return Err(CriticalTopologyError::DuplicateDuty);
        }
        if self.handles[..self.len].iter().flatten().any(|prior| {
            prior.tcb_cap == handle.tcb_cap
                || prior.cnode_cap == handle.cnode_cap
                || prior.sched_context_cap == handle.sched_context_cap
                || prior.reply_cap == handle.reply_cap
                || prior.revoke_anchor_cap == handle.revoke_anchor_cap
        }) {
            return Err(CriticalTopologyError::DuplicateKernelObject);
        }
        let Some(slot) = self.handles.get_mut(self.len) else {
            return Err(CriticalTopologyError::Incomplete);
        };
        *slot = Some(handle);
        self.len += 1;
        Ok(())
    }

    /// Complete only when all five named reserves exist independently.
    pub fn finish(self) -> Result<[CriticalTcbHandle; CRITICAL_TCB_COUNT], CriticalTopologyError> {
        if self.len != CRITICAL_TCB_COUNT {
            return Err(CriticalTopologyError::Incomplete);
        }
        let mut output =
            [self.handles[0].ok_or(CriticalTopologyError::Incomplete)?; CRITICAL_TCB_COUNT];
        for (index, handle) in self.handles.into_iter().enumerate() {
            output[index] = handle.ok_or(CriticalTopologyError::Incomplete)?;
        }
        if REQUIRED_CRITICAL_TCBS
            .iter()
            .any(|id| !output.iter().any(|handle| handle.id == *id))
        {
            return Err(CriticalTopologyError::Incomplete);
        }
        Ok(output)
    }
}

/// Validate the five-duty generated scheduling/fault graph before allocation.
pub fn validate_critical_temporal_graph() -> Result<(), CriticalTopologyError> {
    CriticalHandoff::validate_generated_contract()?;
    FaultRegistry::validate_generated_capacity()
        .map_err(|_| CriticalTopologyError::GeneratedCapacityMismatch)?;
    let tasks = generated::temporal_tasks();
    for id in REQUIRED_CRITICAL_TCBS {
        let task = tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or(CriticalTopologyError::MissingGeneratedTask)?;
        let expected_kind = match id {
            "root-control" => TemporalTaskKind::RootControl,
            "root-fault" => TemporalTaskKind::RootFault,
            "root-emergency" => TemporalTaskKind::RootEmergency,
            "root-worker-supervisor" => TemporalTaskKind::WorkerSupervisor,
            "root-driver-supervisor" => TemporalTaskKind::DriverSupervisor,
            _ => return Err(CriticalTopologyError::TemporalMismatch),
        };
        if task.kind != expected_kind
            || task.execution != TemporalExecution::Active
            || !task.critical_reserve
            || !task.allowed_donors.is_empty()
            || mcs_extra_refills(task.max_refills).is_err()
        {
            return Err(CriticalTopologyError::TemporalMismatch);
        }
        if id == "root-fault" && task.fault_handler != "root-emergency" {
            return Err(CriticalTopologyError::TemporalMismatch);
        }
        if id == "root-emergency" && !task.fault_handler.is_empty() {
            return Err(CriticalTopologyError::TemporalMismatch);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(slot: u16) -> GenerationIdentity {
        GenerationIdentity {
            slot,
            lease_epoch: 1,
            supervisor_generation: 2,
            cap_generation: 3,
        }
    }

    fn fault(worker_ordinal: u16, sequence: u64) -> FaultHandoffRecord {
        let task_index = worker_task_index(worker_ordinal);
        FaultHandoffRecord {
            sequence,
            task_index,
            identity: identity(0),
            fault_badge: 0x26e1_0000 + u64::from(worker_ordinal),
            fault_class: FaultClass::Standard,
            fault_label: 1,
            fault_length: 2,
            fault_mr0: 0,
            fault_mr1: 0,
            tcb_cap: 0x100 + usize::from(worker_ordinal),
        }
    }

    fn worker_task_index(worker_ordinal: u16) -> u16 {
        generated::temporal_tasks()
            .iter()
            .enumerate()
            .filter(|(_, task)| task.kind == TemporalTaskKind::Worker)
            .nth(usize::from(worker_ordinal))
            .and_then(|(index, _)| u16::try_from(index).ok())
            .expect("generated Worker temporal task")
    }

    #[test]
    fn generated_critical_topology_matches_fixed_runtime_bounds() {
        validate_critical_temporal_graph().expect("generated critical topology");
    }

    #[test]
    fn coalesced_worker_drain_prioritizes_faults() {
        let mut handoff = CriticalHandoff::default();
        let controls = WorkerControlQueue::new();
        assert_eq!(
            controls.publish(WorkerControlRecord {
                sequence: 1,
                task_index: worker_task_index(0),
                identity: identity(0),
                operation: WorkerControlOperation::Admit,
            }),
            PublishResult::Published
        );
        handoff.publish_worker_fault(fault(1, 2)).expect("fault");
        assert!(matches!(
            handoff.drain_worker_fault(),
            Some(record) if record.sequence == 2
        ));
        assert!(matches!(
            controls.validate_next().expect("critical validation"),
            Some(record) if record.sequence == 1
        ));
        assert!(matches!(
            controls.drain_validated().expect("root consume"),
            Some(record) if record.sequence == 1
        ));
    }

    #[test]
    fn policy_queue_refuses_and_fault_mailbox_fails_fatal() {
        let mut handoff = CriticalHandoff::default();
        let controls = WorkerControlQueue::new();
        let generated_queue_capacity = u64::from(
            generated::worker_resource_admission_config()
                .handoff
                .worker_control_queue_capacity,
        );
        for sequence in 1..=generated_queue_capacity {
            assert_eq!(
                controls.publish(WorkerControlRecord {
                    sequence,
                    task_index: worker_task_index(0),
                    identity: identity(0),
                    operation: WorkerControlOperation::Admit,
                }),
                PublishResult::Published
            );
        }
        assert_eq!(
            controls.publish(WorkerControlRecord {
                sequence: generated_queue_capacity + 1,
                task_index: worker_task_index(0),
                identity: identity(0),
                operation: WorkerControlOperation::Revoke,
            }),
            PublishResult::Refused
        );
        handoff
            .publish_worker_fault(fault(0, 1))
            .expect("first fault");
        assert_eq!(
            handoff.publish_worker_fault(fault(0, 2)),
            Err(FaultHandoffError::MailboxOccupied)
        );
        assert!(handoff.fatal_fault_handoff());
    }

    #[test]
    fn fault_registry_rejects_duplicates_and_overflow() {
        let generated_capacity = usize::from(
            generated::worker_resource_admission_config()
                .fault_registry
                .capacity,
        );
        let mut registry = FaultRegistry::default();
        for task_index in 0..generated_capacity as u16 {
            let task = &generated::temporal_tasks()[usize::from(task_index)];
            registry
                .register(FaultRegistration {
                    task_index,
                    identity: identity(task_index),
                    standard_badge: generated_standard_fault_badge(task.id)
                        .expect("generated standard badge"),
                    timeout_badge: task.timeout_badge,
                    tcb_cap: 0x3000 + usize::from(task_index),
                    terminal: task.timeout_policy != generated::TimeoutPolicy::ReplenishOnce,
                })
                .expect("register exact TCB");
        }
        assert_eq!(registry.len(), generated_capacity);
        for task_index in 0..generated_capacity as u16 {
            assert!(registry.contains_task_index(task_index));
        }
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len, generated_capacity);
        assert!(!snapshot.sealed);
        assert_eq!(
            snapshot.registration(0).map(|entry| entry.identity),
            Some(identity(0))
        );
        assert!(!registry.contains_task_index(generated_capacity as u16));
        assert_eq!(
            registry.register(FaultRegistration {
                task_index: 99,
                identity: identity(99),
                standard_badge: 0x9990,
                timeout_badge: 0x9991,
                tcb_cap: 0x9992,
                terminal: true,
            }),
            Err(FaultRegistryError::Overflow)
        );
    }

    #[test]
    fn terminal_fault_never_replies_and_recoverable_timeout_replies_once() {
        let registration = FaultRegistration {
            task_index: 1,
            identity: identity(1),
            standard_badge: 0x1001,
            timeout_badge: 0x2001,
            tcb_cap: 0x3001,
            terminal: true,
        };
        let mut terminal = FaultReplyLane::default();
        terminal
            .begin(registration, FaultClass::Standard)
            .expect("associate standard fault");
        assert_eq!(
            terminal.reply_recoverable_timeout(true),
            Err(FaultReplyLaneError::ReplyForbidden)
        );
        terminal
            .finish_terminal(true, true)
            .expect("Suspend clears terminal association");

        let mut timeout = FaultReplyLane::default();
        timeout
            .begin(registration, FaultClass::Timeout)
            .expect("associate timeout");
        timeout
            .reply_recoverable_timeout(true)
            .expect("one allowlisted reply");
        assert_eq!(
            timeout.reply_recoverable_timeout(true),
            Err(FaultReplyLaneError::ReplyAlreadyIssued)
        );
        timeout
            .finish_recoverable(true)
            .expect("reply association clear");
    }

    #[test]
    fn only_ninedoor_owns_the_passive_service_recovery_reply_path() {
        let contract = passive_service_recovery_contract("ninedoor-service")
            .expect("generated passive NineDoor recovery contract");
        assert_eq!(contract.root_fault_reply_slot, 10);
        assert_eq!(contract.mailbox, 0);
        assert_eq!(
            passive_service_recovery_contract("console-network-service"),
            Err(PassiveServiceRecoveryContractError::ActiveOrUnownedService)
        );
    }
}
