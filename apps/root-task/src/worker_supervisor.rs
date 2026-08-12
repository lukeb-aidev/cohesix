// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Supervise isolated Worker children with transactional generation fencing.
// Author: Lukas Bower

//! The supervisor owns the only executable slot for each mandatory Worker role.
//! Construction is transactional, READY is a durable ABI record rather than a
//! successful control write, and every terminal path requires a complete
//! backend containment proof before a slot can be reused.

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
use crate::critical_tcb::{
    FaultHandoffRecord, WorkerControlRecord as CriticalWorkerControlRecord,
    WORKER_CONTROL_QUEUE_CAPACITY, WORKER_FAULT_MAILBOX_CAPACITY,
};
use crate::generated::{self, TemporalTaskConfig, WorkerTaskAbiConfig};
use crate::hal::worker_image::{
    WorkerImagePlan, WORKER_IMAGE_IDENTITIES, WORKER_IMAGE_IDENTITY_BOUND,
};
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
use core::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
use heapless::Deque;
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
use spin::Mutex;
use worker_task_abi::{
    Digest32, GpuLeaseReceiptRecord, PeftReceiptRecord, WorkerAbiError, WorkerCompletionRecord,
    WorkerCompletionStatus, WorkerControlRecord, WorkerIdentity, WorkerLifecycleBits,
    WorkerReadyRecord, WorkerRole, WorkerRuntimeInit,
};

const EXECUTABLE_ROLE_COUNT: usize = 3;

/// Persistent role selector used by one-unit root-control Worker service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkerDeadlineCursor {
    next: WorkerRole,
}

impl Default for WorkerDeadlineCursor {
    fn default() -> Self {
        Self {
            next: WorkerRole::Heartbeat,
        }
    }
}

impl WorkerDeadlineCursor {
    pub(crate) fn take(&mut self) -> WorkerRole {
        let role = self.next;
        self.next = match role {
            WorkerRole::Heartbeat => WorkerRole::Gpu,
            WorkerRole::Gpu => WorkerRole::Lora,
            WorkerRole::Lora => WorkerRole::Heartbeat,
        };
        role
    }
}

/// One bounded item transferred by the restricted Worker-supervisor TCB to
/// root-control. The child owns wake validation and precedence; root-control
/// remains the only thread with object-construction and teardown authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub enum TargetSupervisorWork {
    /// A durable Worker fault record requiring containment.
    Fault(FaultHandoffRecord),
    /// One or more coalesced role-completion notification bits.
    Wake(u64),
    /// An admitted root-control lifecycle operation.
    Control(CriticalWorkerControlRecord),
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
struct TargetSupervisorMailbox {
    faults: [Option<FaultHandoffRecord>; WORKER_FAULT_MAILBOX_CAPACITY],
    controls: Deque<CriticalWorkerControlRecord, WORKER_CONTROL_QUEUE_CAPACITY>,
    wake_bits: u64,
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
impl TargetSupervisorMailbox {
    const fn new() -> Self {
        Self {
            faults: [None; WORKER_FAULT_MAILBOX_CAPACITY],
            controls: Deque::new(),
            wake_bits: 0,
        }
    }

    fn reset(&mut self) {
        self.faults.fill(None);
        self.controls.clear();
        self.wake_bits = 0;
    }

    fn take(&mut self) -> Option<TargetSupervisorWork> {
        for mailbox in &mut self.faults {
            if let Some(record) = mailbox.take() {
                return Some(TargetSupervisorWork::Fault(record));
            }
        }
        if self.wake_bits != 0 {
            let badge = self.wake_bits;
            self.wake_bits = 0;
            return Some(TargetSupervisorWork::Wake(badge));
        }
        self.controls.pop_front().map(TargetSupervisorWork::Control)
    }
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
static TARGET_SUPERVISOR_MAILBOX: Mutex<TargetSupervisorMailbox> =
    Mutex::new(TargetSupervisorMailbox::new());
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
static TARGET_SUPERVISOR_READY: AtomicBool = AtomicBool::new(false);

/// Public lifecycle vocabulary shared by root, console, and host tools.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerLifecycleState {
    /// No instance or retained terminal record occupies the slot.
    Absent,
    /// Root accepted bounded queue admission but has not allocated objects.
    Queued,
    /// Objects exist and root is waiting for an exact durable READY record.
    Starting,
    /// The child is executable and may receive one admitted control record.
    Ready,
    /// New authority is closed and shutdown/teardown is in progress.
    Closing,
    /// A protocol/kernel fault is being contained.
    Faulted,
    /// Containment completed; the terminal identity remains observable.
    Terminal,
}

impl WorkerLifecycleState {
    /// Canonical lowercase label used on operator and host projections.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Closing => "closing",
            Self::Faulted => "faulted",
            Self::Terminal => "terminal",
        }
    }
}

/// Construction phase used for deterministic fault injection and diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerConstructionPhase {
    /// Retype the retained per-slot object bundle beneath its revoke anchor.
    Allocate,
    /// Map/copy the admitted W^X image, stack, IPC buffer, and shared page.
    Map,
    /// Install exact CSpace views and publish the sealed init descriptor.
    Configure,
    /// Bind the compiler-admitted active MCS scheduling context.
    Admit,
    /// Resume the configured child TCB.
    Resume,
}

/// Terminal reason retained with the slot after containment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerTerminalReason {
    /// Operator/root requested a graceful shutdown.
    Shutdown,
    /// Capability authority was explicitly revoked.
    Revoked,
    /// READY was not published within its generated bound.
    ReadyTimeout,
    /// Graceful shutdown exceeded its generated bound.
    ShutdownTimeout,
    /// Root observed a standard child fault.
    Fault,
    /// Root observed a generated timeout fault.
    Timeout,
    /// The child reported invalid control or panic containment.
    ProtocolFault,
    /// Construction failed after one or more objects were allocated.
    ConstructionFailure,
}

/// Exact child-side cap/notification contract installed by the backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerChildContract {
    /// Worker ABI version.
    pub abi_version: u16,
    /// One shared-page size.
    pub shared_page_bytes: u32,
    /// Child IPC-buffer virtual address.
    pub ipc_buffer_vaddr: u64,
    /// Receive-only lifecycle notification CPtr in the child CSpace.
    pub lifecycle_notification_slot: u32,
    /// Send-only supervisor wake CPtr in the child CSpace.
    pub supervisor_wake_notification_slot: u32,
    /// Four one-hot lifecycle causes.
    pub lifecycle_bits: WorkerLifecycleBits,
    /// One-hot role/slot completion bit minted to this child only.
    pub supervisor_wake_bit: u64,
    /// READY deadline relative to resume.
    pub ready_timeout_ms: u32,
    /// Graceful shutdown deadline.
    pub shutdown_grace_ms: u32,
}

/// Proof returned by the backend before a generation can be reclaimed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkerContainmentProof {
    /// TCB was suspended or already non-runnable.
    pub tcb_suspended: bool,
    /// Durable records and pending notification state were cleared.
    pub records_cleared: bool,
    /// Active scheduling context was unbound and no donation is stranded.
    pub scheduling_context_unbound: bool,
    /// Image, stack, IPC, and shared frames were unmapped and scrubbed.
    pub mappings_scrubbed: bool,
    /// Descendants below the retained anchor were revoked.
    pub descendants_revoked: bool,
    /// TCB/CNode/VSpace/SC/endpoint/notification objects were deleted.
    pub objects_deleted: bool,
    /// Old generation badges and cap views can no longer mutate root state.
    pub generation_fenced: bool,
}

impl WorkerContainmentProof {
    /// Return true only for a complete Milestone 26e teardown.
    #[must_use]
    pub const fn complete(self) -> bool {
        self.tcb_suspended
            && self.records_cleared
            && self.scheduling_context_unbound
            && self.mappings_scrubbed
            && self.descendants_revoked
            && self.objects_deleted
            && self.generation_fenced
    }
}

/// Backend boundary implemented by the seL4/HAL object constructor.
pub trait WorkerKernelBackend {
    /// Opaque retained object-bundle handle.
    type Bundle: Copy;

    /// Allocate one complete bundle beneath a retained per-slot revoke anchor.
    /// On error, the backend must internally revoke any partial allocation.
    fn allocate(
        &mut self,
        identity: WorkerIdentity,
        contract: WorkerChildContract,
    ) -> Result<Self::Bundle, WorkerSupervisorError>;

    /// Map/copy one already admitted image using the plan's exact W^X rights.
    fn map_image(
        &mut self,
        bundle: Self::Bundle,
        plan: &WorkerImagePlan,
        image: &[u8],
    ) -> Result<(), WorkerSupervisorError>;

    /// Publish the shared init page and install exact CSpace/VSpace/TCB state.
    fn configure(
        &mut self,
        bundle: Self::Bundle,
        init: WorkerRuntimeInit,
        entry_vaddr: u64,
    ) -> Result<(), WorkerSupervisorError>;

    /// Bind/configure the generated active SC with no SchedControl child cap.
    fn admit(
        &mut self,
        bundle: Self::Bundle,
        temporal: TemporalTaskConfig,
    ) -> Result<(), WorkerSupervisorError>;

    /// Resume the fully configured child.
    fn resume(&mut self, bundle: Self::Bundle) -> Result<(), WorkerSupervisorError>;

    /// Publish one durable control and then signal its one-hot control cause.
    fn publish_control(
        &mut self,
        bundle: Self::Bundle,
        control: WorkerControlRecord,
        control_bit: u64,
    ) -> Result<(), WorkerSupervisorError>;

    /// Signal a generated one-hot shutdown or revoke cause.
    fn signal_lifecycle(
        &mut self,
        bundle: Self::Bundle,
        badge: u64,
    ) -> Result<(), WorkerSupervisorError>;

    /// Execute the full suspend/clear/unbind/scrub/revoke/delete sequence.
    fn contain(
        &mut self,
        bundle: Self::Bundle,
        identity: WorkerIdentity,
        reason: WorkerTerminalReason,
    ) -> Result<WorkerContainmentProof, WorkerSupervisorError>;
}

/// Supervisor refusal or containment failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerSupervisorError {
    /// The selected profile does not admit executable Workers.
    NotEnabled,
    /// WorkerBus or an unknown role was requested.
    RoleNotExecutable,
    /// The role's sole executable slot is already occupied.
    SlotBusy,
    /// Lease, supervisor, or capability generation is zero/stale/exhausted.
    InvalidGeneration,
    /// Image role/digest/entry or W^X plan differs from compiler truth.
    InvalidImage,
    /// A state transition is invalid or a late record targeted an old slot.
    InvalidState,
    /// A READY/control/completion ABI record failed exact validation.
    InvalidRecord,
    /// One-in-flight control admission is already occupied.
    ControlBusy,
    /// No control operation is pending for the supplied completion.
    NoControlPending,
    /// Backend construction or signaling failed.
    Backend,
    /// Teardown did not prove every authority/mapping/SC/object was removed.
    ContainmentIncomplete,
}

impl From<WorkerAbiError> for WorkerSupervisorError {
    fn from(_error: WorkerAbiError) -> Self {
        Self::InvalidRecord
    }
}

/// Reset and publish the bounded mailbox before the restricted supervisor TCB
/// is resumed. This does not transfer HAL or CSpace authority to that child.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn prepare_target_supervisor_mailbox() -> Result<(), WorkerSupervisorError> {
    TARGET_SUPERVISOR_READY.store(false, Ordering::Release);
    let Some(mut mailbox) = TARGET_SUPERVISOR_MAILBOX.try_lock() else {
        return Err(WorkerSupervisorError::Backend);
    };
    mailbox.reset();
    drop(mailbox);
    TARGET_SUPERVISOR_READY.store(true, Ordering::Release);
    Ok(())
}

/// Close target mailbox admission and discard every not-yet-consumed wake.
/// Callers use this only after the restricted supervisor TCB is suspended.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn clear_target_supervisor_mailbox() -> Result<(), WorkerSupervisorError> {
    TARGET_SUPERVISOR_READY.store(false, Ordering::Release);
    let Some(mut mailbox) = TARGET_SUPERVISOR_MAILBOX.try_lock() else {
        return Err(WorkerSupervisorError::Backend);
    };
    mailbox.reset();
    Ok(())
}

/// Whether the root-owned backend mailbox is installed before child startup.
#[must_use]
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn target_supervisor_startup_ready() -> bool {
    TARGET_SUPERVISOR_READY.load(Ordering::Acquire)
}

/// Validate and retain coalesced Worker completion badges without treating a
/// notification as completion evidence. Root-control later reads the durable
/// sequence-last records from the shared pages.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn drain_target_wake(badge: u64) -> Result<(), WorkerSupervisorError> {
    if !target_supervisor_startup_ready() || badge == 0 {
        return Err(WorkerSupervisorError::InvalidState);
    }
    let config = generated::worker_resource_admission_config();
    let abi = generated::worker_runtime_config().task_abi;
    let role_mask = abi.heartbeat_wake_bit | abi.gpu_wake_bit | abi.lora_wake_bit;
    let allowed = config.handoff.worker_wake_badge | role_mask;
    if badge & !allowed != 0 {
        return Err(WorkerSupervisorError::InvalidRecord);
    }
    let role_wakes = badge & role_mask;
    let Some(mut mailbox) = TARGET_SUPERVISOR_MAILBOX.try_lock() else {
        return Err(WorkerSupervisorError::Backend);
    };
    mailbox.wake_bits |= role_wakes;
    Ok(())
}

/// Transfer one fault record from the independently scheduled critical child
/// into its exact durable mailbox. Overwrite is a containment failure.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn drain_critical_fault(record: FaultHandoffRecord) -> Result<(), WorkerSupervisorError> {
    if !target_supervisor_startup_ready() {
        return Err(WorkerSupervisorError::InvalidState);
    }
    let slot = crate::critical_tcb::worker_fault_mailbox_index(record.task_index)
        .ok_or(WorkerSupervisorError::InvalidRecord)?;
    let Some(mut mailbox) = TARGET_SUPERVISOR_MAILBOX.try_lock() else {
        return Err(WorkerSupervisorError::Backend);
    };
    let target = mailbox
        .faults
        .get_mut(slot)
        .ok_or(WorkerSupervisorError::InvalidRecord)?;
    if target.is_some() {
        return Err(WorkerSupervisorError::ContainmentIncomplete);
    }
    *target = Some(record);
    Ok(())
}

/// Transfer one already admitted lifecycle operation into the bounded policy
/// queue. Saturation is an explicit refusal, never a blocking send.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn drain_critical_control(
    record: CriticalWorkerControlRecord,
) -> Result<(), WorkerSupervisorError> {
    if !target_supervisor_startup_ready()
        || record.sequence == 0
        || crate::critical_tcb::worker_fault_mailbox_index(record.task_index).is_none()
        || record.identity.supervisor_generation == 0
        || record.identity.cap_generation == 0
    {
        return Err(WorkerSupervisorError::InvalidRecord);
    }
    let Some(mut mailbox) = TARGET_SUPERVISOR_MAILBOX.try_lock() else {
        return Err(WorkerSupervisorError::Backend);
    };
    mailbox
        .controls
        .push_back(record)
        .map_err(|_| WorkerSupervisorError::ControlBusy)
}

/// Nonblockingly take the next root-control work item in fault, durable-wake,
/// then policy order. A contended mailbox is retried on a later root turn.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn take_target_supervisor_work() -> Result<Option<TargetSupervisorWork>, WorkerSupervisorError>
{
    if !target_supervisor_startup_ready() {
        return Ok(None);
    }
    let Some(mut mailbox) = TARGET_SUPERVISOR_MAILBOX.try_lock() else {
        return Err(WorkerSupervisorError::Backend);
    };
    Ok(mailbox.take())
}

/// Receipt returned when root accepts and constructs one executable slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerSpawnReceipt {
    /// Immutable five-part identity reserved before READY.
    pub identity: WorkerIdentity,
    /// Observable lifecycle after construction/resume.
    pub lifecycle: WorkerLifecycleState,
    /// Absolute READY deadline.
    pub ready_deadline_ms: u64,
}

/// Bounded state projected without exposing raw CPtrs or badges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerSlotSnapshot {
    /// Exact Worker role for this executable slot.
    pub role: WorkerRole,
    /// Current lifecycle.
    pub lifecycle: WorkerLifecycleState,
    /// Current/last immutable identity when present.
    pub identity: Option<WorkerIdentity>,
    /// Last accepted READY sequence.
    pub ready_sequence: u64,
    /// Current in-flight control sequence, or zero.
    pub control_sequence: u64,
    /// Last receipt sequence correlated to the current control, or zero.
    pub receipt_sequence: u64,
    /// Terminal reason retained after teardown.
    pub terminal_reason: Option<WorkerTerminalReason>,
}

struct WorkerSlot<Bundle: Copy> {
    role: WorkerRole,
    lifecycle: WorkerLifecycleState,
    identity: Option<WorkerIdentity>,
    bundle: Option<Bundle>,
    init: Option<WorkerRuntimeInit>,
    last_lease_epoch: u64,
    cap_generation: u64,
    ready_sequence: u64,
    last_control_sequence: u64,
    pending_control: Option<WorkerControlRecord>,
    receipt_sequence: u64,
    ready_deadline_ms: u64,
    shutdown_deadline_ms: u64,
    terminal_reason: Option<WorkerTerminalReason>,
}

impl<Bundle: Copy> WorkerSlot<Bundle> {
    const fn empty(role: WorkerRole) -> Self {
        Self {
            role,
            lifecycle: WorkerLifecycleState::Absent,
            identity: None,
            bundle: None,
            init: None,
            last_lease_epoch: 0,
            cap_generation: 0,
            ready_sequence: 0,
            last_control_sequence: 0,
            pending_control: None,
            receipt_sequence: 0,
            ready_deadline_ms: 0,
            shutdown_deadline_ms: 0,
            terminal_reason: None,
        }
    }

    fn snapshot(&self) -> WorkerSlotSnapshot {
        WorkerSlotSnapshot {
            role: self.role,
            lifecycle: self.lifecycle,
            identity: self.identity,
            ready_sequence: self.ready_sequence,
            control_sequence: self.pending_control.map_or(0, |record| record.sequence),
            receipt_sequence: self.receipt_sequence,
            terminal_reason: self.terminal_reason,
        }
    }
}

/// Transactional supervisor for the exact Heartbeat/GPU/LoRA role matrix.
pub struct WorkerSupervisor<Backend: WorkerKernelBackend> {
    backend: Backend,
    slots: [WorkerSlot<Backend::Bundle>; EXECUTABLE_ROLE_COUNT],
    supervisor_generation: u64,
    descriptor_sequence: u64,
}

impl<Backend: WorkerKernelBackend> WorkerSupervisor<Backend> {
    /// Create an empty supervisor; no authority is live until [`Self::spawn`].
    #[must_use]
    pub const fn new(backend: Backend) -> Self {
        Self {
            backend,
            slots: [
                WorkerSlot::empty(WorkerRole::Heartbeat),
                WorkerSlot::empty(WorkerRole::Gpu),
                WorkerSlot::empty(WorkerRole::Lora),
            ],
            supervisor_generation: 0,
            descriptor_sequence: 0,
        }
    }

    /// Borrow the backend for target diagnostics or tests.
    #[must_use]
    pub const fn backend(&self) -> &Backend {
        &self.backend
    }

    /// Mutably borrow the backend for target wake/record plumbing.
    pub fn backend_mut(&mut self) -> &mut Backend {
        &mut self.backend
    }

    /// Return the bounded public state for one executable role.
    pub fn snapshot(&self, role: WorkerRole) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        Ok(self.slots[role_index(role)?].snapshot())
    }

    /// Return the sealed runtime descriptor for one live generation.
    ///
    /// Root uses this narrow read-only view to reject malformed control
    /// records before admitting them to the target's bounded handoff.
    pub fn runtime_init(
        &self,
        role: WorkerRole,
    ) -> Result<WorkerRuntimeInit, WorkerSupervisorError> {
        self.slots[role_index(role)?]
            .init
            .ok_or(WorkerSupervisorError::InvalidState)
    }

    /// Start the READY deadline when a preconstructed target child is actually
    /// resumed. Target bootstrap may construct a complete generation before
    /// the fault graph is sealed, so its construction-time timestamp is not an
    /// admissible runtime deadline.
    pub fn arm_preconstructed_ready_deadline(
        &mut self,
        role: WorkerRole,
        now_ms: u64,
    ) -> Result<WorkerSpawnReceipt, WorkerSupervisorError> {
        let index = role_index(role)?;
        let slot = &mut self.slots[index];
        if slot.lifecycle != WorkerLifecycleState::Starting {
            return Err(WorkerSupervisorError::InvalidState);
        }
        let identity = slot.identity.ok_or(WorkerSupervisorError::InvalidState)?;
        let timeout_ms =
            child_contract(role, generated::worker_runtime_config().task_abi)?.ready_timeout_ms;
        slot.ready_deadline_ms = now_ms
            .checked_add(u64::from(timeout_ms))
            .ok_or(WorkerSupervisorError::InvalidGeneration)?;
        Ok(WorkerSpawnReceipt {
            identity,
            lifecycle: WorkerLifecycleState::Starting,
            ready_deadline_ms: slot.ready_deadline_ms,
        })
    }

    /// Construct, admit, and resume one role-exact child, pending READY.
    pub fn spawn(
        &mut self,
        role: WorkerRole,
        lease_epoch: u64,
        plan: &WorkerImagePlan,
        image: &[u8],
        now_ms: u64,
    ) -> Result<WorkerSpawnReceipt, WorkerSupervisorError> {
        validate_static_contract(role, plan)?;
        let index = role_index(role)?;
        let slot = &mut self.slots[index];
        if !matches!(
            slot.lifecycle,
            WorkerLifecycleState::Absent | WorkerLifecycleState::Terminal
        ) {
            return Err(WorkerSupervisorError::SlotBusy);
        }
        if lease_epoch == 0 || lease_epoch <= slot.last_lease_epoch {
            return Err(WorkerSupervisorError::InvalidGeneration);
        }
        self.supervisor_generation = self
            .supervisor_generation
            .checked_add(1)
            .ok_or(WorkerSupervisorError::InvalidGeneration)?;
        slot.cap_generation = slot
            .cap_generation
            .checked_add(1)
            .ok_or(WorkerSupervisorError::InvalidGeneration)?;
        self.descriptor_sequence = self
            .descriptor_sequence
            .checked_add(1)
            .ok_or(WorkerSupervisorError::InvalidGeneration)?;
        let identity = WorkerIdentity::new(
            role,
            0,
            lease_epoch,
            self.supervisor_generation,
            slot.cap_generation,
        );
        identity
            .validate_for_role(role)
            .map_err(|_| WorkerSupervisorError::InvalidGeneration)?;
        let task_abi = generated::worker_runtime_config().task_abi;
        let contract = child_contract(role, task_abi)?;
        let contract_digest = generated_manifest_digest()?;
        let init = WorkerRuntimeInit::staged(
            self.descriptor_sequence,
            identity,
            contract.ipc_buffer_vaddr,
            u64::from(contract.lifecycle_notification_slot),
            u64::from(contract.supervisor_wake_notification_slot),
            contract.lifecycle_bits,
            contract.supervisor_wake_bit,
            Digest32::new(plan.image_sha256),
            contract_digest,
        )
        .committed();
        init.validate_for_role(role)?;
        let temporal = temporal_task(role)?;
        slot.lifecycle = WorkerLifecycleState::Queued;
        slot.identity = Some(identity);
        slot.init = Some(init);
        slot.last_lease_epoch = lease_epoch;
        slot.ready_sequence = 0;
        slot.last_control_sequence = 0;
        slot.pending_control = None;
        slot.receipt_sequence = 0;
        slot.terminal_reason = None;

        let bundle = match self.backend.allocate(identity, contract) {
            Ok(bundle) => bundle,
            Err(_) => {
                slot.lifecycle = WorkerLifecycleState::Terminal;
                slot.terminal_reason = Some(WorkerTerminalReason::ConstructionFailure);
                return Err(WorkerSupervisorError::Backend);
            }
        };
        slot.bundle = Some(bundle);
        if self.backend.map_image(bundle, plan, image).is_err() {
            return self.fail_construction(index, identity, bundle);
        }
        if self
            .backend
            .configure(bundle, init, plan.entry_vaddr)
            .is_err()
        {
            return self.fail_construction(index, identity, bundle);
        }
        if self.backend.admit(bundle, temporal).is_err() {
            return self.fail_construction(index, identity, bundle);
        }
        if self.backend.resume(bundle).is_err() {
            return self.fail_construction(index, identity, bundle);
        }
        slot.lifecycle = WorkerLifecycleState::Starting;
        slot.ready_deadline_ms = now_ms
            .checked_add(u64::from(contract.ready_timeout_ms))
            .ok_or(WorkerSupervisorError::InvalidGeneration)?;
        Ok(WorkerSpawnReceipt {
            identity,
            lifecycle: WorkerLifecycleState::Starting,
            ready_deadline_ms: slot.ready_deadline_ms,
        })
    }

    fn fail_construction(
        &mut self,
        index: usize,
        identity: WorkerIdentity,
        bundle: Backend::Bundle,
    ) -> Result<WorkerSpawnReceipt, WorkerSupervisorError> {
        self.slots[index].lifecycle = WorkerLifecycleState::Faulted;
        let proof = self
            .backend
            .contain(bundle, identity, WorkerTerminalReason::ConstructionFailure)
            .map_err(|_| WorkerSupervisorError::ContainmentIncomplete)?;
        if !proof.complete() {
            return Err(WorkerSupervisorError::ContainmentIncomplete);
        }
        let slot = &mut self.slots[index];
        slot.bundle = None;
        slot.init = None;
        slot.lifecycle = WorkerLifecycleState::Terminal;
        slot.terminal_reason = Some(WorkerTerminalReason::ConstructionFailure);
        Err(WorkerSupervisorError::Backend)
    }

    /// Accept READY only from the exact current five-part identity and digests.
    pub fn accept_ready(
        &mut self,
        record: WorkerReadyRecord,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let role = record
            .identity
            .worker_role()
            .map_err(|_| WorkerSupervisorError::InvalidRecord)?;
        let index = role_index(role)?;
        let slot = &mut self.slots[index];
        if slot.lifecycle != WorkerLifecycleState::Starting {
            return Err(WorkerSupervisorError::InvalidState);
        }
        let init = slot.init.ok_or(WorkerSupervisorError::InvalidState)?;
        record.validate_for(init)?;
        if record.sequence <= slot.ready_sequence {
            return Err(WorkerSupervisorError::InvalidRecord);
        }
        slot.ready_sequence = record.sequence;
        slot.lifecycle = WorkerLifecycleState::Ready;
        Ok(slot.snapshot())
    }

    /// Admit one role-correct durable control; a write does not imply READY.
    pub fn submit_control(
        &mut self,
        control: WorkerControlRecord,
    ) -> Result<(), WorkerSupervisorError> {
        let role = control
            .identity
            .worker_role()
            .map_err(|_| WorkerSupervisorError::InvalidRecord)?;
        let index = role_index(role)?;
        let slot = &mut self.slots[index];
        if slot.lifecycle != WorkerLifecycleState::Ready {
            return Err(WorkerSupervisorError::InvalidState);
        }
        if slot.pending_control.is_some() {
            return Err(WorkerSupervisorError::ControlBusy);
        }
        if control.sequence <= slot.last_control_sequence {
            return Err(WorkerSupervisorError::InvalidRecord);
        }
        let init = slot.init.ok_or(WorkerSupervisorError::InvalidState)?;
        control.validate_for(init)?;
        let bundle = slot.bundle.ok_or(WorkerSupervisorError::InvalidState)?;
        self.backend
            .publish_control(bundle, control, init.lifecycle_bits.control)
            .map_err(|_| WorkerSupervisorError::Backend)?;
        slot.last_control_sequence = control.sequence;
        slot.pending_control = Some(control);
        slot.receipt_sequence = 0;
        Ok(())
    }

    /// Accept one exact GPU receipt before the matching completion record.
    pub fn accept_gpu_receipt(
        &mut self,
        receipt: GpuLeaseReceiptRecord,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let index = role_index(WorkerRole::Gpu)?;
        let slot = &mut self.slots[index];
        if slot.lifecycle != WorkerLifecycleState::Ready || slot.receipt_sequence != 0 {
            return Err(WorkerSupervisorError::InvalidState);
        }
        let init = slot.init.ok_or(WorkerSupervisorError::InvalidState)?;
        receipt.validate_for(init)?;
        let control = slot
            .pending_control
            .ok_or(WorkerSupervisorError::NoControlPending)?;
        if receipt.sequence != control.sequence
            || receipt.identity != control.identity
            || receipt.action != control.action
            || receipt.outcome != control.outcome
            || receipt.digests != control.digests
        {
            return Err(WorkerSupervisorError::InvalidRecord);
        }
        slot.receipt_sequence = receipt.sequence;
        Ok(slot.snapshot())
    }

    /// Accept one exact PEFT receipt before the matching completion record.
    pub fn accept_peft_receipt(
        &mut self,
        receipt: PeftReceiptRecord,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let index = role_index(WorkerRole::Lora)?;
        let slot = &mut self.slots[index];
        if slot.lifecycle != WorkerLifecycleState::Ready || slot.receipt_sequence != 0 {
            return Err(WorkerSupervisorError::InvalidState);
        }
        let init = slot.init.ok_or(WorkerSupervisorError::InvalidState)?;
        receipt.validate_for(init)?;
        let control = slot
            .pending_control
            .ok_or(WorkerSupervisorError::NoControlPending)?;
        if receipt.sequence != control.sequence
            || receipt.identity != control.identity
            || receipt.action != control.action
            || receipt.outcome != control.outcome
            || receipt.digests != control.digests
        {
            return Err(WorkerSupervisorError::InvalidRecord);
        }
        slot.receipt_sequence = receipt.sequence;
        Ok(slot.snapshot())
    }

    /// Accept one exact completion or terminal child report.
    pub fn accept_completion(
        &mut self,
        completion: WorkerCompletionRecord,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let role = completion
            .identity
            .worker_role()
            .map_err(|_| WorkerSupervisorError::InvalidRecord)?;
        let index = role_index(role)?;
        let init = self.slots[index]
            .init
            .ok_or(WorkerSupervisorError::InvalidState)?;
        completion.validate_for(init)?;
        let status = WorkerCompletionStatus::from_raw(completion.status)?;
        match status {
            WorkerCompletionStatus::Confirmed
            | WorkerCompletionStatus::Rejected
            | WorkerCompletionStatus::Stale => {
                let slot = &mut self.slots[index];
                let control = slot
                    .pending_control
                    .ok_or(WorkerSupervisorError::NoControlPending)?;
                if slot.lifecycle != WorkerLifecycleState::Ready
                    || control.sequence != completion.sequence
                    || control.action != completion.action
                    || control.identity != completion.identity
                    || control.digests.result != completion.result_digest
                    || (role != WorkerRole::Heartbeat
                        && (slot.receipt_sequence != completion.sequence
                            || control.outcome != completion.status))
                {
                    return Err(WorkerSupervisorError::NoControlPending);
                }
                slot.pending_control = None;
                slot.receipt_sequence = 0;
                Ok(slot.snapshot())
            }
            WorkerCompletionStatus::Shutdown => {
                if self.slots[index].lifecycle != WorkerLifecycleState::Closing {
                    return self.protocol_fault(index);
                }
                self.finish_containment(index, WorkerTerminalReason::Shutdown)
            }
            WorkerCompletionStatus::Revoked => {
                self.finish_containment(index, WorkerTerminalReason::Revoked)
            }
            WorkerCompletionStatus::Timeout => {
                self.finish_containment(index, WorkerTerminalReason::Timeout)
            }
            WorkerCompletionStatus::InvalidControl | WorkerCompletionStatus::Panic => {
                self.finish_containment(index, WorkerTerminalReason::ProtocolFault)
            }
        }
    }

    fn protocol_fault(
        &mut self,
        index: usize,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let _ = self.finish_containment(index, WorkerTerminalReason::ProtocolFault)?;
        Err(WorkerSupervisorError::InvalidRecord)
    }

    /// Stop new authority immediately and request bounded graceful shutdown.
    pub fn begin_shutdown(
        &mut self,
        role: WorkerRole,
        now_ms: u64,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let index = role_index(role)?;
        let slot = &mut self.slots[index];
        if slot.lifecycle == WorkerLifecycleState::Closing {
            return Ok(slot.snapshot());
        }
        if !matches!(
            slot.lifecycle,
            WorkerLifecycleState::Starting | WorkerLifecycleState::Ready
        ) {
            return Err(WorkerSupervisorError::InvalidState);
        }
        let init = slot.init.ok_or(WorkerSupervisorError::InvalidState)?;
        let bundle = slot.bundle.ok_or(WorkerSupervisorError::InvalidState)?;
        slot.lifecycle = WorkerLifecycleState::Closing;
        slot.pending_control = None;
        slot.receipt_sequence = 0;
        slot.shutdown_deadline_ms = now_ms
            .checked_add(u64::from(
                generated::worker_runtime_config()
                    .task_abi
                    .shutdown_grace_ms,
            ))
            .ok_or(WorkerSupervisorError::InvalidGeneration)?;
        self.backend
            .signal_lifecycle(bundle, init.lifecycle_bits.shutdown)
            .map_err(|_| WorkerSupervisorError::Backend)?;
        Ok(slot.snapshot())
    }

    /// Revoke immediately and contain without waiting for child cooperation.
    pub fn revoke(
        &mut self,
        role: WorkerRole,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let index = role_index(role)?;
        let slot = &self.slots[index];
        let init = slot.init.ok_or(WorkerSupervisorError::InvalidState)?;
        let bundle = slot.bundle.ok_or(WorkerSupervisorError::InvalidState)?;
        let _ = self
            .backend
            .signal_lifecycle(bundle, init.lifecycle_bits.revoke);
        self.finish_containment(index, WorkerTerminalReason::Revoked)
    }

    /// Attribute a standard/timeout fault to the exact current identity.
    pub fn fault(
        &mut self,
        identity: WorkerIdentity,
        timeout: bool,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let role = identity
            .worker_role()
            .map_err(|_| WorkerSupervisorError::InvalidRecord)?;
        let index = role_index(role)?;
        if self.slots[index].identity != Some(identity) {
            return Err(WorkerSupervisorError::InvalidGeneration);
        }
        self.finish_containment(
            index,
            if timeout {
                WorkerTerminalReason::Timeout
            } else {
                WorkerTerminalReason::Fault
            },
        )
    }

    /// Enforce generated READY and shutdown deadlines without retry loops.
    pub fn enforce_deadlines(&mut self, now_ms: u64) -> Result<usize, WorkerSupervisorError> {
        let mut contained = 0usize;
        for index in 0..self.slots.len() {
            contained += usize::from(self.enforce_deadline_index(index, now_ms)?);
        }
        Ok(contained)
    }

    /// Enforce the generated deadline for one exact executable role.
    ///
    /// Target bootstrap uses this to leave unclaimed, preconstructed children
    /// suspended without aging their READY deadlines.
    pub fn enforce_role_deadline(
        &mut self,
        role: WorkerRole,
        now_ms: u64,
    ) -> Result<bool, WorkerSupervisorError> {
        self.enforce_deadline_index(role_index(role)?, now_ms)
    }

    fn enforce_deadline_index(
        &mut self,
        index: usize,
        now_ms: u64,
    ) -> Result<bool, WorkerSupervisorError> {
        let reason = match self.slots[index].lifecycle {
            WorkerLifecycleState::Starting if now_ms >= self.slots[index].ready_deadline_ms => {
                Some(WorkerTerminalReason::ReadyTimeout)
            }
            WorkerLifecycleState::Closing if now_ms >= self.slots[index].shutdown_deadline_ms => {
                Some(WorkerTerminalReason::ShutdownTimeout)
            }
            _ => None,
        };
        if let Some(reason) = reason {
            self.finish_containment(index, reason)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn finish_containment(
        &mut self,
        index: usize,
        reason: WorkerTerminalReason,
    ) -> Result<WorkerSlotSnapshot, WorkerSupervisorError> {
        let identity = self.slots[index]
            .identity
            .ok_or(WorkerSupervisorError::InvalidState)?;
        let bundle = self.slots[index]
            .bundle
            .ok_or(WorkerSupervisorError::InvalidState)?;
        self.slots[index].lifecycle = WorkerLifecycleState::Faulted;
        let proof = self
            .backend
            .contain(bundle, identity, reason)
            .map_err(|_| WorkerSupervisorError::ContainmentIncomplete)?;
        if !proof.complete() {
            return Err(WorkerSupervisorError::ContainmentIncomplete);
        }
        let slot = &mut self.slots[index];
        slot.bundle = None;
        slot.init = None;
        slot.pending_control = None;
        slot.receipt_sequence = 0;
        slot.ready_deadline_ms = 0;
        slot.shutdown_deadline_ms = 0;
        slot.lifecycle = WorkerLifecycleState::Terminal;
        slot.terminal_reason = Some(reason);
        Ok(slot.snapshot())
    }
}

fn role_index(role: WorkerRole) -> Result<usize, WorkerSupervisorError> {
    Ok(match role {
        WorkerRole::Heartbeat => 0,
        WorkerRole::Gpu => 1,
        WorkerRole::Lora => 2,
    })
}

fn role_label(role: WorkerRole) -> &'static str {
    match role {
        WorkerRole::Heartbeat => "worker-heartbeat",
        WorkerRole::Gpu => "worker-gpu",
        WorkerRole::Lora => "worker-lora",
    }
}

fn image_name(role: WorkerRole) -> &'static str {
    match role {
        WorkerRole::Heartbeat => "worker-heart",
        WorkerRole::Gpu => "worker-gpu",
        WorkerRole::Lora => "worker-lora",
    }
}

fn validate_static_contract(
    role: WorkerRole,
    plan: &WorkerImagePlan,
) -> Result<(), WorkerSupervisorError> {
    let runtime = generated::worker_runtime_config();
    let admission = generated::worker_resource_admission_config();
    if !runtime.cap_backed_authority
        || !runtime.notification_lifecycle
        || !runtime.task_abi.enabled
        || !admission.enabled
    {
        return Err(WorkerSupervisorError::NotEnabled);
    }
    let label = role_label(role);
    if !runtime
        .roles
        .iter()
        .any(|record| record.implemented && role_matches(record.role, role))
        || !admission
            .executable_roles
            .iter()
            .any(|record| record.role == label && record.executable_slots == 1)
    {
        return Err(WorkerSupervisorError::RoleNotExecutable);
    }
    if plan.expected.name != image_name(role)
        || plan.expected.role != label
        || plan.segment_count == 0
        || WORKER_IMAGE_IDENTITY_BOUND && !WORKER_IMAGE_IDENTITIES.contains(&plan.expected)
    {
        return Err(WorkerSupervisorError::InvalidImage);
    }
    Ok(())
}

fn role_matches(role: cohesix_ticket::Role, expected: WorkerRole) -> bool {
    matches!(
        (role, expected),
        (cohesix_ticket::Role::WorkerHeartbeat, WorkerRole::Heartbeat)
            | (cohesix_ticket::Role::WorkerGpu, WorkerRole::Gpu)
            | (cohesix_ticket::Role::WorkerLora, WorkerRole::Lora)
    )
}

fn child_contract(
    role: WorkerRole,
    config: WorkerTaskAbiConfig,
) -> Result<WorkerChildContract, WorkerSupervisorError> {
    if !config.enabled
        || config.version != 1
        || config.shared_page_bytes != 4096
        || config.max_control_inflight != 1
    {
        return Err(WorkerSupervisorError::NotEnabled);
    }
    let lifecycle_bits = WorkerLifecycleBits {
        control: config.lifecycle_control_bit,
        timeout: config.lifecycle_timeout_bit,
        shutdown: config.lifecycle_shutdown_bit,
        revoke: config.lifecycle_revoke_bit,
    };
    lifecycle_bits.validate()?;
    let supervisor_wake_bit = match role {
        WorkerRole::Heartbeat => config.heartbeat_wake_bit,
        WorkerRole::Gpu => config.gpu_wake_bit,
        WorkerRole::Lora => config.lora_wake_bit,
    };
    if !supervisor_wake_bit.is_power_of_two() || lifecycle_bits.mask() & supervisor_wake_bit != 0 {
        return Err(WorkerSupervisorError::InvalidRecord);
    }
    Ok(WorkerChildContract {
        abi_version: config.version,
        shared_page_bytes: config.shared_page_bytes,
        ipc_buffer_vaddr: config.ipc_buffer_vaddr,
        lifecycle_notification_slot: config.lifecycle_notification_slot,
        supervisor_wake_notification_slot: config.supervisor_wake_notification_slot,
        lifecycle_bits,
        supervisor_wake_bit,
        ready_timeout_ms: config.ready_timeout_ms,
        shutdown_grace_ms: config.shutdown_grace_ms,
    })
}

fn temporal_task(role: WorkerRole) -> Result<TemporalTaskConfig, WorkerSupervisorError> {
    let id = match role {
        WorkerRole::Heartbeat => "worker-heartbeat-slot-0",
        WorkerRole::Gpu => "worker-gpu-slot-0",
        WorkerRole::Lora => "worker-lora-slot-0",
    };
    generated::temporal_tasks()
        .iter()
        .find(|task| task.id == id)
        .copied()
        .ok_or(WorkerSupervisorError::NotEnabled)
}

fn generated_manifest_digest() -> Result<Digest32, WorkerSupervisorError> {
    let value = generated::MANIFEST_SHA256.as_bytes();
    if value.len() != 64 {
        return Err(WorkerSupervisorError::NotEnabled);
    }
    let mut bytes = [0u8; 32];
    for (index, pair) in value.chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0]).ok_or(WorkerSupervisorError::NotEnabled)?;
        let low = hex_nibble(pair[1]).ok_or(WorkerSupervisorError::NotEnabled)?;
        bytes[index] = high << 4 | low;
    }
    let digest = Digest32::new(bytes);
    if digest.is_zero() {
        return Err(WorkerSupervisorError::NotEnabled);
    }
    Ok(digest)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkerDeadlineCursor, WorkerRole};

    #[test]
    fn isolated_runtime_deadline_cursor_retains_role_rotation() {
        let mut cursor = WorkerDeadlineCursor::default();

        assert_eq!(cursor.take(), WorkerRole::Heartbeat);
        assert_eq!(cursor.take(), WorkerRole::Gpu);
        assert_eq!(cursor.take(), WorkerRole::Lora);
        assert_eq!(cursor.take(), WorkerRole::Heartbeat);
    }
}
