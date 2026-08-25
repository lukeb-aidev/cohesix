// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define the pointer-free fixed-layout ABI for isolated Worker tasks.
// Author: Lukas Bower

#![no_std]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! `worker-task-abi/v2` records shared by an isolated Worker and its supervisor.
//!
//! Records are pointer-free, fixed-layout values. Producers stage the complete
//! body with a zero commit word, execute a release fence, write the final
//! `committed_sequence`, and only then signal the peer. Consumers acquire the
//! commit word, copy the body, and accept it only when both sequence reads are
//! equal. The memory access itself belongs to the mapped-runtime transport;
//! this crate defines the values and their fail-closed validation.

use core::mem::{align_of, offset_of, size_of};

/// Runtime-init magic (`WKI2`).
pub const WORKER_RUNTIME_INIT_MAGIC: u32 = 0x574b_4932;
/// Control-record magic (`WKC2`).
pub const WORKER_CONTROL_MAGIC: u32 = 0x574b_4332;
/// Completion-record magic (`WKO2`).
pub const WORKER_COMPLETION_MAGIC: u32 = 0x574b_4f32;
/// READY-record magic (`WKR2`).
pub const WORKER_READY_MAGIC: u32 = 0x574b_5232;
/// GPU receipt magic (`WKG2`).
pub const WORKER_GPU_RECEIPT_MAGIC: u32 = 0x574b_4732;
/// PEFT receipt magic (`WKP2`).
pub const WORKER_PEFT_RECEIPT_MAGIC: u32 = 0x574b_5032;
/// Worker image metadata magic (`WKM2`).
pub const WORKER_IMAGE_METADATA_MAGIC: u32 = 0x574b_4d32;
/// ABI version shared by all Worker records.
pub const WORKER_TASK_ABI_VERSION: u16 = 2;
/// Version of the declared Worker executable entry contract.
pub const WORKER_IMAGE_ENTRY_VERSION: u16 = 2;
/// Retained ELF section containing one [`WorkerImageMetadata`] record.
pub const WORKER_IMAGE_METADATA_SECTION: &str = ".cohesix.worker";
/// Fixed bytes reserved for the declared executable entry symbol.
pub const WORKER_IMAGE_ENTRY_SYMBOL_BYTES: usize = 32;
/// Exact entry symbol for Worker task ABI version 1.
pub const WORKER_IMAGE_ENTRY_SYMBOL: [u8; WORKER_IMAGE_ENTRY_SYMBOL_BYTES] =
    fixed_entry_symbol(b"_start");
/// Bytes in the one-page Worker ABI mapping.
pub const WORKER_SHARED_PAGE_BYTES: usize = 4096;
/// Required shared-page alignment.
pub const WORKER_SHARED_PAGE_ALIGNMENT: usize = 4096;
/// Minimum seL4 IPC-buffer alignment accepted by the runtime descriptor.
pub const WORKER_IPC_BUFFER_ALIGNMENT: u64 = 1024;
/// Fixed byte capacity of receipt labels and schema names.
pub const WORKER_RECEIPT_LABEL_BYTES: usize = 24;

/// Exact GPU receipt label.
pub const GPU_LEASE_RECEIPT: [u8; WORKER_RECEIPT_LABEL_BYTES] = fixed_label(b"GPU_LEASE_RECEIPT");
/// Exact PEFT receipt label.
pub const PEFT_RECEIPT: [u8; WORKER_RECEIPT_LABEL_BYTES] = fixed_label(b"PEFT_RECEIPT");
/// Canonical bounded GPU receipt schema label.
pub const WORKER_GPU_RECEIPT_SCHEMA: [u8; WORKER_RECEIPT_LABEL_BYTES] =
    fixed_label(b"worker-gpu-receipt/v1");
/// Canonical bounded LoRA receipt schema label.
pub const WORKER_LORA_RECEIPT_SCHEMA: [u8; WORKER_RECEIPT_LABEL_BYTES] =
    fixed_label(b"worker-lora-receipt/v1");

/// Runtime init is pointer-free and uses only durable records and notifications.
pub const WORKER_RUNTIME_FLAG_POINTER_FREE: u32 = 1 << 0;
/// Control and completion records remain authoritative across coalesced wakes.
pub const WORKER_RUNTIME_FLAG_DURABLE_RECORDS: u32 = 1 << 1;
/// Every producer commits records by writing the sequence last.
pub const WORKER_RUNTIME_FLAG_SEQUENCE_LAST: u32 = 1 << 2;
/// Normal Worker execution uses bounded passive endpoint Call/Reply.
pub const WORKER_RUNTIME_FLAG_PASSIVE_CALL_REPLY: u32 = 1 << 3;
/// Every passive request has exactly one donor and one Reply object.
pub const WORKER_RUNTIME_FLAG_DEPTH_ONE_DONATION: u32 = 1 << 4;
/// Exact required runtime-init flags.
pub const WORKER_RUNTIME_REQUIRED_FLAGS: u32 = WORKER_RUNTIME_FLAG_POINTER_FREE
    | WORKER_RUNTIME_FLAG_DURABLE_RECORDS
    | WORKER_RUNTIME_FLAG_SEQUENCE_LAST
    | WORKER_RUNTIME_FLAG_PASSIVE_CALL_REPLY
    | WORKER_RUNTIME_FLAG_DEPTH_ONE_DONATION;

/// Ordinary successful executor/Worker reply label.
pub const WORKER_CALL_SUCCESS_LABEL: u64 = 0;
/// Typed recovery reply emitted by root-fault after a Worker failure.
pub const WORKER_CALL_RECOVERED_LABEL: u64 = 0x26ef_0001;

/// Worker image metadata contains no addresses or architecture-sized fields.
pub const WORKER_IMAGE_FLAG_POINTER_FREE: u32 = 1 << 0;
/// The declared entry receives the shared runtime-init page address in `x0`.
pub const WORKER_IMAGE_FLAG_INIT_PAGE_IN_X0: u32 = 1 << 1;
/// Exact required flags for a Worker image metadata record.
pub const WORKER_IMAGE_REQUIRED_FLAGS: u32 =
    WORKER_IMAGE_FLAG_POINTER_FREE | WORKER_IMAGE_FLAG_INIT_PAGE_IN_X0;

const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

const fn fixed_label(value: &[u8]) -> [u8; WORKER_RECEIPT_LABEL_BYTES] {
    let mut output = [0u8; WORKER_RECEIPT_LABEL_BYTES];
    let mut index = 0usize;
    while index < value.len() && index < output.len() {
        output[index] = value[index];
        index += 1;
    }
    output
}

const fn fixed_entry_symbol(value: &[u8]) -> [u8; WORKER_IMAGE_ENTRY_SYMBOL_BYTES] {
    let mut output = [0u8; WORKER_IMAGE_ENTRY_SYMBOL_BYTES];
    let mut index = 0usize;
    while index < value.len() && index < output.len() {
        output[index] = value[index];
        index += 1;
    }
    output
}

const fn bytes_equal<const N: usize>(left: &[u8; N], right: &[u8; N]) -> bool {
    let mut index = 0usize;
    while index < N {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

const fn bytes_are_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0usize;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

const fn hash_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(FNV64_PRIME)
}

const fn hash_u16(mut hash: u64, value: u16) -> u64 {
    let mut shift = 0;
    while shift < 16 {
        hash = hash_byte(hash, ((value >> shift) & 0xff) as u8);
        shift += 8;
    }
    hash
}

const fn hash_u32(mut hash: u64, value: u32) -> u64 {
    let mut shift = 0;
    while shift < 32 {
        hash = hash_byte(hash, ((value >> shift) & 0xff) as u8);
        shift += 8;
    }
    hash
}

const fn hash_u64(mut hash: u64, value: u64) -> u64 {
    let mut shift = 0;
    while shift < 64 {
        hash = hash_byte(hash, ((value >> shift) & 0xff) as u8);
        shift += 8;
    }
    hash
}

const fn hash_digest(mut hash: u64, digest: Digest32) -> u64 {
    let mut index = 0usize;
    while index < digest.bytes.len() {
        hash = hash_byte(hash, digest.bytes[index]);
        index += 1;
    }
    hash
}

/// Validation failures for ABI records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerAbiError {
    /// A record magic value is not the expected record type.
    InvalidMagic,
    /// The ABI version is not exactly version 2.
    InvalidVersion,
    /// The declared record length differs from its fixed layout.
    InvalidLength,
    /// Reserved fields or runtime flags are not exact.
    InvalidFlags,
    /// The Worker role is unknown or inappropriate for the record.
    InvalidRole,
    /// A Worker identity has a zero or mismatched component.
    InvalidIdentity,
    /// A descriptor sequence is zero, stale, or not committed sequence-last.
    InvalidSequence,
    /// An init descriptor seal does not bind its complete body.
    InvalidSeal,
    /// A mapped-page or IPC-buffer bound is invalid.
    InvalidMapping,
    /// A declared capability slot is zero, aliased, or out of range.
    InvalidCapabilitySlot,
    /// A call label is zero, aliased, or unknown.
    InvalidCallLabel,
    /// A control or receipt action is unknown or incompatible with its role.
    InvalidAction,
    /// A terminal result outcome is missing or invalid.
    InvalidOutcome,
    /// A required SHA-256 digest is absent or an unexpected digest is present.
    InvalidDigest,
    /// A receipt label or canonical schema label is not exact.
    InvalidReceiptLabel,
    /// A Worker image does not declare the exact ABI entry symbol.
    InvalidEntrySymbol,
    /// A time bound is reversed.
    InvalidDeadline,
}

/// Exact executable Worker roles in ABI version 2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum WorkerRole {
    /// Heartbeat telemetry projection.
    Heartbeat = 1,
    /// GPU lease-result receipt projection.
    Gpu = 2,
    /// PEFT lifecycle-result receipt projection.
    Lora = 3,
}

impl WorkerRole {
    /// Decode a raw role, rejecting model-only or unknown roles.
    pub const fn from_raw(raw: u16) -> Result<Self, WorkerAbiError> {
        match raw {
            1 => Ok(Self::Heartbeat),
            2 => Ok(Self::Gpu),
            3 => Ok(Self::Lora),
            _ => Err(WorkerAbiError::InvalidRole),
        }
    }
}

/// Retained, pointer-free identity record for one executable Worker image.
///
/// Exactly one record is emitted into the read-only `.cohesix.worker` ELF
/// section of every Worker target binary. Image admission validates this
/// record before the supervisor creates the task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(8))]
pub struct WorkerImageMetadata {
    /// [`WORKER_IMAGE_METADATA_MAGIC`].
    pub magic: u32,
    /// Exact [`WORKER_TASK_ABI_VERSION`].
    pub abi_version: u16,
    /// Fixed byte length of this record.
    pub metadata_length: u16,
    /// Exact raw [`WorkerRole`] discriminant for this image.
    pub role: u16,
    /// Exact [`WORKER_IMAGE_ENTRY_VERSION`].
    pub entry_version: u16,
    /// Exact [`WORKER_IMAGE_REQUIRED_FLAGS`].
    pub flags: u32,
    /// NUL-padded [`WORKER_IMAGE_ENTRY_SYMBOL`].
    pub entry_symbol: [u8; WORKER_IMAGE_ENTRY_SYMBOL_BYTES],
    /// Reserved for compatible readers; must be zero.
    pub reserved: [u8; 16],
}

impl WorkerImageMetadata {
    /// Construct the exact metadata record for an executable Worker role.
    #[must_use]
    pub const fn for_role(role: WorkerRole) -> Self {
        Self {
            magic: WORKER_IMAGE_METADATA_MAGIC,
            abi_version: WORKER_TASK_ABI_VERSION,
            metadata_length: size_of::<Self>() as u16,
            role: role as u16,
            entry_version: WORKER_IMAGE_ENTRY_VERSION,
            flags: WORKER_IMAGE_REQUIRED_FLAGS,
            entry_symbol: WORKER_IMAGE_ENTRY_SYMBOL,
            reserved: [0; 16],
        }
    }

    /// Validate all fixed fields and return the declared executable role.
    pub const fn validate(self) -> Result<WorkerRole, WorkerAbiError> {
        if self.magic != WORKER_IMAGE_METADATA_MAGIC {
            return Err(WorkerAbiError::InvalidMagic);
        }
        if self.abi_version != WORKER_TASK_ABI_VERSION
            || self.entry_version != WORKER_IMAGE_ENTRY_VERSION
        {
            return Err(WorkerAbiError::InvalidVersion);
        }
        if self.metadata_length as usize != size_of::<Self>() {
            return Err(WorkerAbiError::InvalidLength);
        }
        let role = match WorkerRole::from_raw(self.role) {
            Ok(role) => role,
            Err(error) => return Err(error),
        };
        if self.flags != WORKER_IMAGE_REQUIRED_FLAGS || !bytes_are_zero(&self.reserved) {
            return Err(WorkerAbiError::InvalidFlags);
        }
        if !bytes_equal(&self.entry_symbol, &WORKER_IMAGE_ENTRY_SYMBOL) {
            return Err(WorkerAbiError::InvalidEntrySymbol);
        }
        Ok(role)
    }

    /// Validate all fixed fields and the exact expected executable role.
    pub const fn validate_for_role(self, role: WorkerRole) -> Result<(), WorkerAbiError> {
        match self.validate() {
            Ok(declared_role) if declared_role as u16 == role as u16 => Ok(()),
            Ok(_) => Err(WorkerAbiError::InvalidRole),
            Err(error) => Err(error),
        }
    }
}

/// Full generation-bound identity of one executable Worker instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct WorkerIdentity {
    /// Raw [`WorkerRole`] discriminant.
    pub role: u16,
    /// Reserved; must be zero.
    pub reserved: u16,
    /// Manifest-declared slot within the role.
    pub slot: u32,
    /// Lease epoch admitted by root.
    pub lease_epoch: u64,
    /// Generation of the root-owned supervisor instance.
    pub supervisor_generation: u64,
    /// Generation of the capability bundle installed into the child.
    pub cap_generation: u64,
}

impl WorkerIdentity {
    /// Construct an identity. Call [`Self::validate`] before admission.
    #[must_use]
    pub const fn new(
        role: WorkerRole,
        slot: u32,
        lease_epoch: u64,
        supervisor_generation: u64,
        cap_generation: u64,
    ) -> Self {
        Self {
            role: role as u16,
            reserved: 0,
            slot,
            lease_epoch,
            supervisor_generation,
            cap_generation,
        }
    }

    /// Return the decoded executable role.
    pub const fn worker_role(self) -> Result<WorkerRole, WorkerAbiError> {
        WorkerRole::from_raw(self.role)
    }

    /// Validate all five identity components.
    pub const fn validate(self) -> Result<(), WorkerAbiError> {
        if self.reserved != 0
            || self.worker_role().is_err()
            || self.lease_epoch == 0
            || self.supervisor_generation == 0
            || self.cap_generation == 0
        {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        Ok(())
    }

    /// Validate this identity for one executable role.
    pub const fn validate_for_role(self, role: WorkerRole) -> Result<(), WorkerAbiError> {
        if self.validate().is_err() || self.role != role as u16 {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        Ok(())
    }
}

/// Fixed SHA-256 digest storage used by the Worker ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Digest32 {
    /// Digest bytes in canonical SHA-256 byte order.
    pub bytes: [u8; 32],
}

impl Digest32 {
    /// All-zero sentinel used only where the action forbids a digest.
    pub const ZERO: Self = Self { bytes: [0; 32] };

    /// Construct a digest from exact bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Return true when every byte is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        let mut index = 0usize;
        while index < self.bytes.len() {
            if self.bytes[index] != 0 {
                return false;
            }
            index += 1;
        }
        true
    }
}

/// Digests binding a terminal host action to an exact Worker receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct ReceiptDigests {
    /// SHA-256 digest of the canonical ticket id.
    pub ticket: Digest32,
    /// SHA-256 digest of the canonical idempotency key.
    pub idempotency: Digest32,
    /// SHA-256 digest of the operation id.
    pub operation: Digest32,
    /// SHA-256 digest of the root-normalized subject reference.
    pub subject: Digest32,
    /// SHA-256 digest of the canonical terminal result envelope.
    pub result: Digest32,
}

impl ReceiptDigests {
    /// Empty digest set for actions that do not emit host-result receipts.
    pub const EMPTY: Self = Self {
        ticket: Digest32::ZERO,
        idempotency: Digest32::ZERO,
        operation: Digest32::ZERO,
        subject: Digest32::ZERO,
        result: Digest32::ZERO,
    };

    /// Return true when all receipt-bearing digests are populated.
    #[must_use]
    pub const fn complete(self) -> bool {
        !self.ticket.is_zero()
            && !self.idempotency.is_zero()
            && !self.operation.is_zero()
            && !self.subject.is_zero()
            && !self.result.is_zero()
    }

    /// Return true when no digest is present.
    #[must_use]
    pub const fn empty(self) -> bool {
        self.ticket.is_zero()
            && self.idempotency.is_zero()
            && self.operation.is_zero()
            && self.subject.is_zero()
            && self.result.is_zero()
    }
}

/// Root-admitted action codes. Workers project results and never execute them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum WorkerAction {
    /// Emit one root-admitted heartbeat sample.
    HeartbeatPublish = 0x0101,
    /// Project a terminal `gpu.lease.grant` result.
    GpuLeaseGrant = 0x0201,
    /// Project a terminal `gpu.lease.renew` result.
    GpuLeaseRenew = 0x0202,
    /// Project a terminal `gpu.lease.release` result.
    GpuLeaseRelease = 0x0203,
    /// Project a terminal `peft.export` result.
    PeftExport = 0x0301,
    /// Project a terminal `peft.import` result.
    PeftImport = 0x0302,
    /// Project a terminal `peft.activate` result.
    PeftActivate = 0x0303,
    /// Project a terminal `peft.rollback` result.
    PeftRollback = 0x0304,
}

impl WorkerAction {
    /// Decode an exact action code.
    pub const fn from_raw(raw: u16) -> Result<Self, WorkerAbiError> {
        match raw {
            0x0101 => Ok(Self::HeartbeatPublish),
            0x0201 => Ok(Self::GpuLeaseGrant),
            0x0202 => Ok(Self::GpuLeaseRenew),
            0x0203 => Ok(Self::GpuLeaseRelease),
            0x0301 => Ok(Self::PeftExport),
            0x0302 => Ok(Self::PeftImport),
            0x0303 => Ok(Self::PeftActivate),
            0x0304 => Ok(Self::PeftRollback),
            _ => Err(WorkerAbiError::InvalidAction),
        }
    }

    /// Return the sole Worker role permitted to project this action.
    #[must_use]
    pub const fn role(self) -> WorkerRole {
        match self {
            Self::HeartbeatPublish => WorkerRole::Heartbeat,
            Self::GpuLeaseGrant | Self::GpuLeaseRenew | Self::GpuLeaseRelease => WorkerRole::Gpu,
            Self::PeftExport | Self::PeftImport | Self::PeftActivate | Self::PeftRollback => {
                WorkerRole::Lora
            }
        }
    }

    /// Return true for the three receipt-bearing GPU actions.
    #[must_use]
    pub const fn is_gpu(self) -> bool {
        matches!(
            self,
            Self::GpuLeaseGrant | Self::GpuLeaseRenew | Self::GpuLeaseRelease
        )
    }

    /// Return true for the four receipt-bearing PEFT actions.
    #[must_use]
    pub const fn is_peft(self) -> bool {
        matches!(
            self,
            Self::PeftExport | Self::PeftImport | Self::PeftActivate | Self::PeftRollback
        )
    }
}

/// Terminal outcome supplied by root after host-side execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum WorkerOutcome {
    /// No host result is applicable, as for a heartbeat publication.
    NotApplicable = 0,
    /// The host result was terminal and confirmed.
    Confirmed = 1,
    /// The host result was terminal and rejected.
    Rejected = 2,
    /// The otherwise valid terminal result targeted a retired Worker identity.
    Stale = 8,
}

impl WorkerOutcome {
    /// Decode an exact outcome code.
    pub const fn from_raw(raw: u16) -> Result<Self, WorkerAbiError> {
        match raw {
            0 => Ok(Self::NotApplicable),
            1 => Ok(Self::Confirmed),
            2 => Ok(Self::Rejected),
            8 => Ok(Self::Stale),
            _ => Err(WorkerAbiError::InvalidOutcome),
        }
    }

    /// Return true for a terminal host-result outcome.
    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Rejected | Self::Stale)
    }
}

/// Exact synchronous operation labels accepted by one passive Worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct WorkerCallLabels {
    /// Durable control record is available.
    pub control: u64,
    /// Graceful shutdown was requested.
    pub shutdown: u64,
    /// Current authority was revoked.
    pub revoke: u64,
}

impl WorkerCallLabels {
    /// Validate nonzero, pairwise-disjoint call labels.
    pub const fn validate(self) -> Result<(), WorkerAbiError> {
        let labels = [self.control, self.shutdown, self.revoke];
        let mut index = 0usize;
        while index < labels.len() {
            if labels[index] == 0 {
                return Err(WorkerAbiError::InvalidCallLabel);
            }
            let mut prior = 0usize;
            while prior < index {
                if labels[prior] == labels[index] {
                    return Err(WorkerAbiError::InvalidCallLabel);
                }
                prior += 1;
            }
            index += 1;
        }
        Ok(())
    }

    /// Decode one exact request label; labels never coalesce.
    pub const fn classify(self, label: u64) -> Result<WorkerCallOperation, WorkerAbiError> {
        if self.validate().is_err() {
            return Err(WorkerAbiError::InvalidCallLabel);
        }
        if label == self.control {
            Ok(WorkerCallOperation::Control)
        } else if label == self.shutdown {
            Ok(WorkerCallOperation::Shutdown)
        } else if label == self.revoke {
            Ok(WorkerCallOperation::Revoke)
        } else {
            Err(WorkerAbiError::InvalidCallLabel)
        }
    }
}

/// Exact operation selected from a synchronous call label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerCallOperation {
    /// Process the durable control record.
    Control,
    /// Publish graceful shutdown completion and await teardown.
    Shutdown,
    /// Publish revocation completion and await teardown.
    Revoke,
}

/// Sealed runtime descriptor supplied in the first entry register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct WorkerRuntimeInit {
    /// [`WORKER_RUNTIME_INIT_MAGIC`].
    pub magic: u32,
    /// [`WORKER_TASK_ABI_VERSION`].
    pub version: u16,
    /// Exact descriptor length.
    pub length: u16,
    /// Exact [`WORKER_RUNTIME_REQUIRED_FLAGS`].
    pub flags: u32,
    /// Reserved; must be zero.
    pub reserved0: u32,
    /// Nonzero descriptor publication sequence.
    pub descriptor_sequence: u64,
    /// Immutable five-part Worker identity.
    pub identity: WorkerIdentity,
    /// Exact size of [`WorkerSharedPage`].
    pub shared_page_bytes: u32,
    /// Reserved; must be zero.
    pub reserved1: u32,
    /// Mapped seL4 IPC-buffer virtual address.
    pub ipc_buffer_vaddr: u64,
    /// Receive-only service endpoint slot.
    pub service_endpoint_slot: u64,
    /// Single-owner service Reply-object slot.
    pub service_reply_slot: u64,
    /// Send-only supervisor wake notification slot.
    pub supervisor_wake_notification_slot: u64,
    /// Generated synchronous operation labels.
    pub call_labels: WorkerCallLabels,
    /// Exact badge minted on the executor's Call cap.
    pub request_badge: u64,
    /// One-hot badge minted on the supervisor wake cap.
    pub supervisor_wake_bit: u64,
    /// SHA-256 digest of the exact Worker image.
    pub image_digest: Digest32,
    /// SHA-256 digest of the generated Worker contract.
    pub contract_digest: Digest32,
    /// Deterministic seal over every preceding semantic field.
    pub seal: u64,
    /// Sequence-last commit; must equal `descriptor_sequence`.
    pub committed_sequence: u64,
}

impl WorkerRuntimeInit {
    /// Construct a sealed but uncommitted runtime descriptor.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "fixed-layout ABI construction mirrors generated descriptor fields"
    )]
    pub const fn staged(
        descriptor_sequence: u64,
        identity: WorkerIdentity,
        ipc_buffer_vaddr: u64,
        service_endpoint_slot: u64,
        service_reply_slot: u64,
        supervisor_wake_notification_slot: u64,
        call_labels: WorkerCallLabels,
        request_badge: u64,
        supervisor_wake_bit: u64,
        image_digest: Digest32,
        contract_digest: Digest32,
    ) -> Self {
        let mut descriptor = Self {
            magic: WORKER_RUNTIME_INIT_MAGIC,
            version: WORKER_TASK_ABI_VERSION,
            length: size_of::<Self>() as u16,
            flags: WORKER_RUNTIME_REQUIRED_FLAGS,
            reserved0: 0,
            descriptor_sequence,
            identity,
            shared_page_bytes: WORKER_SHARED_PAGE_BYTES as u32,
            reserved1: 0,
            ipc_buffer_vaddr,
            service_endpoint_slot,
            service_reply_slot,
            supervisor_wake_notification_slot,
            call_labels,
            request_badge,
            supervisor_wake_bit,
            image_digest,
            contract_digest,
            seal: 0,
            committed_sequence: 0,
        };
        descriptor.seal = descriptor.expected_seal();
        descriptor
    }

    /// Return the descriptor with its sequence-last commit populated.
    #[must_use]
    pub const fn committed(mut self) -> Self {
        self.committed_sequence = self.descriptor_sequence;
        self
    }

    /// Compute the deterministic descriptor seal without hashing padding.
    #[must_use]
    pub const fn expected_seal(self) -> u64 {
        let mut hash = hash_u32(FNV64_OFFSET, self.magic);
        hash = hash_u16(hash, self.version);
        hash = hash_u16(hash, self.length);
        hash = hash_u32(hash, self.flags);
        hash = hash_u32(hash, self.reserved0);
        hash = hash_u64(hash, self.descriptor_sequence);
        hash = hash_u16(hash, self.identity.role);
        hash = hash_u16(hash, self.identity.reserved);
        hash = hash_u32(hash, self.identity.slot);
        hash = hash_u64(hash, self.identity.lease_epoch);
        hash = hash_u64(hash, self.identity.supervisor_generation);
        hash = hash_u64(hash, self.identity.cap_generation);
        hash = hash_u32(hash, self.shared_page_bytes);
        hash = hash_u32(hash, self.reserved1);
        hash = hash_u64(hash, self.ipc_buffer_vaddr);
        hash = hash_u64(hash, self.service_endpoint_slot);
        hash = hash_u64(hash, self.service_reply_slot);
        hash = hash_u64(hash, self.supervisor_wake_notification_slot);
        hash = hash_u64(hash, self.call_labels.control);
        hash = hash_u64(hash, self.call_labels.shutdown);
        hash = hash_u64(hash, self.call_labels.revoke);
        hash = hash_u64(hash, self.request_badge);
        hash = hash_u64(hash, self.supervisor_wake_bit);
        hash = hash_digest(hash, self.image_digest);
        hash = hash_digest(hash, self.contract_digest);
        if hash == 0 {
            WORKER_RUNTIME_INIT_MAGIC as u64
        } else {
            hash
        }
    }

    /// Validate the descriptor and its expected executable role.
    pub const fn validate_for_role(self, role: WorkerRole) -> Result<(), WorkerAbiError> {
        if self.magic != WORKER_RUNTIME_INIT_MAGIC {
            return Err(WorkerAbiError::InvalidMagic);
        }
        if self.version != WORKER_TASK_ABI_VERSION {
            return Err(WorkerAbiError::InvalidVersion);
        }
        if self.length as usize != size_of::<Self>() {
            return Err(WorkerAbiError::InvalidLength);
        }
        if self.flags != WORKER_RUNTIME_REQUIRED_FLAGS || self.reserved0 != 0 || self.reserved1 != 0
        {
            return Err(WorkerAbiError::InvalidFlags);
        }
        if self.descriptor_sequence == 0 || self.committed_sequence != self.descriptor_sequence {
            return Err(WorkerAbiError::InvalidSequence);
        }
        if self.identity.validate_for_role(role).is_err() {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        if self.shared_page_bytes as usize != WORKER_SHARED_PAGE_BYTES
            || self.ipc_buffer_vaddr == 0
            || !self
                .ipc_buffer_vaddr
                .is_multiple_of(WORKER_IPC_BUFFER_ALIGNMENT)
        {
            return Err(WorkerAbiError::InvalidMapping);
        }
        if self.service_endpoint_slot == 0
            || self.service_reply_slot == 0
            || self.supervisor_wake_notification_slot == 0
            || self.service_endpoint_slot == self.service_reply_slot
            || self.service_endpoint_slot == self.supervisor_wake_notification_slot
            || self.service_reply_slot == self.supervisor_wake_notification_slot
            || self.service_endpoint_slot > u32::MAX as u64
            || self.service_reply_slot > u32::MAX as u64
            || self.supervisor_wake_notification_slot > u32::MAX as u64
        {
            return Err(WorkerAbiError::InvalidCapabilitySlot);
        }
        if self.call_labels.validate().is_err()
            || self.request_badge == 0
            || self.supervisor_wake_bit == 0
            || !self.supervisor_wake_bit.is_power_of_two()
        {
            return Err(WorkerAbiError::InvalidCallLabel);
        }
        if self.image_digest.is_zero() || self.contract_digest.is_zero() {
            return Err(WorkerAbiError::InvalidDigest);
        }
        if self.seal == 0 || self.seal != self.expected_seal() {
            return Err(WorkerAbiError::InvalidSeal);
        }
        Ok(())
    }
}

/// One durable command published by root to a Worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct WorkerControlRecord {
    /// [`WORKER_CONTROL_MAGIC`].
    pub magic: u32,
    /// [`WORKER_TASK_ABI_VERSION`].
    pub version: u16,
    /// Exact record length.
    pub length: u16,
    /// Reserved; must be zero.
    pub flags: u32,
    /// Reserved; must be zero.
    pub reserved0: u32,
    /// Monotonic root-assigned control sequence.
    pub sequence: u64,
    /// Full Worker identity pinned at admission.
    pub identity: WorkerIdentity,
    /// Raw [`WorkerAction`] code.
    pub action: u16,
    /// Raw [`WorkerOutcome`] supplied for terminal host results.
    pub outcome: u16,
    /// Reserved; must be zero.
    pub reserved1: u32,
    /// Root-admitted monotonic event time.
    pub admitted_time_ns: u64,
    /// Root-admitted deadline, or zero when no deadline applies.
    pub deadline_ns: u64,
    /// Exact receipt digests; empty only for heartbeat publication.
    pub digests: ReceiptDigests,
    /// Sequence-last commit; must equal `sequence`.
    pub committed_sequence: u64,
}

impl WorkerControlRecord {
    /// Empty, uncommitted record.
    pub const EMPTY: Self = Self {
        magic: WORKER_CONTROL_MAGIC,
        version: WORKER_TASK_ABI_VERSION,
        length: size_of::<Self>() as u16,
        flags: 0,
        reserved0: 0,
        sequence: 0,
        identity: WorkerIdentity {
            role: 0,
            reserved: 0,
            slot: 0,
            lease_epoch: 0,
            supervisor_generation: 0,
            cap_generation: 0,
        },
        action: 0,
        outcome: 0,
        reserved1: 0,
        admitted_time_ns: 0,
        deadline_ns: 0,
        digests: ReceiptDigests::EMPTY,
        committed_sequence: 0,
    };

    /// Construct a staged control body.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "fixed-layout ABI construction mirrors the control body"
    )]
    pub const fn staged(
        sequence: u64,
        identity: WorkerIdentity,
        action: WorkerAction,
        outcome: WorkerOutcome,
        admitted_time_ns: u64,
        deadline_ns: u64,
        digests: ReceiptDigests,
    ) -> Self {
        Self {
            magic: WORKER_CONTROL_MAGIC,
            version: WORKER_TASK_ABI_VERSION,
            length: size_of::<Self>() as u16,
            flags: 0,
            reserved0: 0,
            sequence,
            identity,
            action: action as u16,
            outcome: outcome as u16,
            reserved1: 0,
            admitted_time_ns,
            deadline_ns,
            digests,
            committed_sequence: 0,
        }
    }

    /// Return the record with its commit sequence populated.
    #[must_use]
    pub const fn committed(mut self) -> Self {
        self.committed_sequence = self.sequence;
        self
    }

    /// Decode the admitted action.
    pub const fn worker_action(self) -> Result<WorkerAction, WorkerAbiError> {
        WorkerAction::from_raw(self.action)
    }

    /// Decode the admitted outcome.
    pub const fn worker_outcome(self) -> Result<WorkerOutcome, WorkerAbiError> {
        WorkerOutcome::from_raw(self.outcome)
    }

    /// Validate a stable committed record against its sealed init descriptor.
    pub fn validate_for(self, init: WorkerRuntimeInit) -> Result<(), WorkerAbiError> {
        if self.magic != WORKER_CONTROL_MAGIC {
            return Err(WorkerAbiError::InvalidMagic);
        }
        if self.version != WORKER_TASK_ABI_VERSION {
            return Err(WorkerAbiError::InvalidVersion);
        }
        if self.length as usize != size_of::<Self>() {
            return Err(WorkerAbiError::InvalidLength);
        }
        if self.flags != 0 || self.reserved0 != 0 || self.reserved1 != 0 {
            return Err(WorkerAbiError::InvalidFlags);
        }
        if self.sequence == 0 || self.committed_sequence != self.sequence {
            return Err(WorkerAbiError::InvalidSequence);
        }
        if self.identity != init.identity || self.identity.validate().is_err() {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        let action = self.worker_action()?;
        let role = self.identity.worker_role()?;
        if action.role() != role {
            return Err(WorkerAbiError::InvalidAction);
        }
        let outcome = self.worker_outcome()?;
        if action == WorkerAction::HeartbeatPublish {
            if outcome != WorkerOutcome::NotApplicable || !self.digests.empty() {
                return Err(WorkerAbiError::InvalidOutcome);
            }
        } else if !outcome.terminal() || !self.digests.complete() {
            return Err(if !outcome.terminal() {
                WorkerAbiError::InvalidOutcome
            } else {
                WorkerAbiError::InvalidDigest
            });
        }
        if self.deadline_ns != 0 && self.deadline_ns < self.admitted_time_ns {
            return Err(WorkerAbiError::InvalidDeadline);
        }
        Ok(())
    }
}

/// Completion status published by a Worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum WorkerCompletionStatus {
    /// The control result was confirmed.
    Confirmed = 1,
    /// The control result was rejected.
    Rejected = 2,
    /// A committed control record failed strict validation.
    InvalidControl = 3,
    /// The current operation timed out.
    Timeout = 4,
    /// Graceful shutdown was observed.
    Shutdown = 5,
    /// Capability authority was revoked.
    Revoked = 6,
    /// The Worker panicked and entered its fault path.
    Panic = 7,
    /// The correlated result was valid but its pinned Worker identity retired.
    Stale = 8,
}

impl WorkerCompletionStatus {
    /// Decode an exact completion status.
    pub const fn from_raw(raw: u16) -> Result<Self, WorkerAbiError> {
        match raw {
            1 => Ok(Self::Confirmed),
            2 => Ok(Self::Rejected),
            3 => Ok(Self::InvalidControl),
            4 => Ok(Self::Timeout),
            5 => Ok(Self::Shutdown),
            6 => Ok(Self::Revoked),
            7 => Ok(Self::Panic),
            8 => Ok(Self::Stale),
            _ => Err(WorkerAbiError::InvalidOutcome),
        }
    }
}

/// Durable Worker completion record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct WorkerCompletionRecord {
    /// [`WORKER_COMPLETION_MAGIC`].
    pub magic: u32,
    /// [`WORKER_TASK_ABI_VERSION`].
    pub version: u16,
    /// Exact record length.
    pub length: u16,
    /// Reserved; must be zero.
    pub flags: u32,
    /// Reserved; must be zero.
    pub reserved0: u32,
    /// Monotonic completion sequence.
    pub sequence: u64,
    /// Full identity of the publishing Worker.
    pub identity: WorkerIdentity,
    /// Action code, or zero for asynchronous lifecycle/fault completion.
    pub action: u16,
    /// Raw [`WorkerCompletionStatus`].
    pub status: u16,
    /// Reserved; must be zero.
    pub reserved1: u32,
    /// Canonical terminal result digest, or zero when not applicable.
    pub result_digest: Digest32,
    /// Sequence-last commit; must equal `sequence`.
    pub committed_sequence: u64,
}

impl WorkerCompletionRecord {
    /// Empty, uncommitted record.
    pub const EMPTY: Self = Self {
        magic: WORKER_COMPLETION_MAGIC,
        version: WORKER_TASK_ABI_VERSION,
        length: size_of::<Self>() as u16,
        flags: 0,
        reserved0: 0,
        sequence: 0,
        identity: WorkerControlRecord::EMPTY.identity,
        action: 0,
        status: 0,
        reserved1: 0,
        result_digest: Digest32::ZERO,
        committed_sequence: 0,
    };

    /// Build a staged completion for one validated control record.
    #[must_use]
    pub const fn staged_for_control(control: WorkerControlRecord) -> Self {
        let status = match control.outcome {
            value if value == WorkerOutcome::Rejected as u16 => WorkerCompletionStatus::Rejected,
            value if value == WorkerOutcome::Stale as u16 => WorkerCompletionStatus::Stale,
            _ => WorkerCompletionStatus::Confirmed,
        };
        Self {
            magic: WORKER_COMPLETION_MAGIC,
            version: WORKER_TASK_ABI_VERSION,
            length: size_of::<Self>() as u16,
            flags: 0,
            reserved0: 0,
            sequence: control.sequence,
            identity: control.identity,
            action: control.action,
            status: status as u16,
            reserved1: 0,
            result_digest: control.digests.result,
            committed_sequence: 0,
        }
    }

    /// Build a staged asynchronous lifecycle or fault completion.
    #[must_use]
    pub const fn staged_terminal(
        sequence: u64,
        identity: WorkerIdentity,
        status: WorkerCompletionStatus,
    ) -> Self {
        Self {
            magic: WORKER_COMPLETION_MAGIC,
            version: WORKER_TASK_ABI_VERSION,
            length: size_of::<Self>() as u16,
            flags: 0,
            reserved0: 0,
            sequence,
            identity,
            action: 0,
            status: status as u16,
            reserved1: 0,
            result_digest: Digest32::ZERO,
            committed_sequence: 0,
        }
    }

    /// Return the record with its commit sequence populated.
    #[must_use]
    pub const fn committed(mut self) -> Self {
        self.committed_sequence = self.sequence;
        self
    }

    /// Validate a stable committed completion.
    pub fn validate_for(self, init: WorkerRuntimeInit) -> Result<(), WorkerAbiError> {
        if self.magic != WORKER_COMPLETION_MAGIC {
            return Err(WorkerAbiError::InvalidMagic);
        }
        if self.version != WORKER_TASK_ABI_VERSION {
            return Err(WorkerAbiError::InvalidVersion);
        }
        if self.length as usize != size_of::<Self>() {
            return Err(WorkerAbiError::InvalidLength);
        }
        if self.flags != 0 || self.reserved0 != 0 || self.reserved1 != 0 {
            return Err(WorkerAbiError::InvalidFlags);
        }
        if self.sequence == 0 || self.committed_sequence != self.sequence {
            return Err(WorkerAbiError::InvalidSequence);
        }
        if self.identity != init.identity || self.identity.validate().is_err() {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        let status = WorkerCompletionStatus::from_raw(self.status)?;
        match status {
            WorkerCompletionStatus::Confirmed
            | WorkerCompletionStatus::Rejected
            | WorkerCompletionStatus::Stale => {
                let action = WorkerAction::from_raw(self.action)?;
                if action.role() as u16 != self.identity.role {
                    return Err(WorkerAbiError::InvalidAction);
                }
                if action == WorkerAction::HeartbeatPublish {
                    if !self.result_digest.is_zero() {
                        return Err(WorkerAbiError::InvalidDigest);
                    }
                } else if self.result_digest.is_zero() {
                    return Err(WorkerAbiError::InvalidDigest);
                }
            }
            WorkerCompletionStatus::InvalidControl
            | WorkerCompletionStatus::Timeout
            | WorkerCompletionStatus::Shutdown
            | WorkerCompletionStatus::Revoked
            | WorkerCompletionStatus::Panic => {
                if self.action != 0 || !self.result_digest.is_zero() {
                    return Err(WorkerAbiError::InvalidAction);
                }
            }
        }
        Ok(())
    }
}

/// Durable READY record published once after init validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct WorkerReadyRecord {
    /// [`WORKER_READY_MAGIC`].
    pub magic: u32,
    /// [`WORKER_TASK_ABI_VERSION`].
    pub version: u16,
    /// Exact record length.
    pub length: u16,
    /// Reserved; must be zero.
    pub flags: u32,
    /// Reserved; must be zero.
    pub reserved0: u32,
    /// Nonzero READY publication sequence.
    pub sequence: u64,
    /// Full identity of the ready Worker.
    pub identity: WorkerIdentity,
    /// Exact Worker image digest copied from init.
    pub image_digest: Digest32,
    /// Exact generated contract digest copied from init.
    pub contract_digest: Digest32,
    /// Sequence-last commit; must equal `sequence`.
    pub committed_sequence: u64,
}

impl WorkerReadyRecord {
    /// Empty, uncommitted READY record.
    pub const EMPTY: Self = Self {
        magic: WORKER_READY_MAGIC,
        version: WORKER_TASK_ABI_VERSION,
        length: size_of::<Self>() as u16,
        flags: 0,
        reserved0: 0,
        sequence: 0,
        identity: WorkerControlRecord::EMPTY.identity,
        image_digest: Digest32::ZERO,
        contract_digest: Digest32::ZERO,
        committed_sequence: 0,
    };

    /// Construct a staged READY record from a validated init descriptor.
    #[must_use]
    pub const fn staged(init: WorkerRuntimeInit, sequence: u64) -> Self {
        Self {
            magic: WORKER_READY_MAGIC,
            version: WORKER_TASK_ABI_VERSION,
            length: size_of::<Self>() as u16,
            flags: 0,
            reserved0: 0,
            sequence,
            identity: init.identity,
            image_digest: init.image_digest,
            contract_digest: init.contract_digest,
            committed_sequence: 0,
        }
    }

    /// Return the record with its commit sequence populated.
    #[must_use]
    pub const fn committed(mut self) -> Self {
        self.committed_sequence = self.sequence;
        self
    }

    /// Validate a stable committed READY record.
    pub fn validate_for(self, init: WorkerRuntimeInit) -> Result<(), WorkerAbiError> {
        if self.magic != WORKER_READY_MAGIC {
            return Err(WorkerAbiError::InvalidMagic);
        }
        if self.version != WORKER_TASK_ABI_VERSION {
            return Err(WorkerAbiError::InvalidVersion);
        }
        if self.length as usize != size_of::<Self>() {
            return Err(WorkerAbiError::InvalidLength);
        }
        if self.flags != 0 || self.reserved0 != 0 {
            return Err(WorkerAbiError::InvalidFlags);
        }
        if self.sequence == 0 || self.committed_sequence != self.sequence {
            return Err(WorkerAbiError::InvalidSequence);
        }
        if self.identity != init.identity {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        if self.image_digest != init.image_digest || self.contract_digest != init.contract_digest {
            return Err(WorkerAbiError::InvalidDigest);
        }
        Ok(())
    }
}

/// Exact fixed-layout GPU lease receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct GpuLeaseReceiptRecord {
    /// [`WORKER_GPU_RECEIPT_MAGIC`].
    pub magic: u32,
    /// [`WORKER_TASK_ABI_VERSION`].
    pub version: u16,
    /// Exact record length.
    pub length: u16,
    /// Exact [`GPU_LEASE_RECEIPT`] label.
    pub label: [u8; WORKER_RECEIPT_LABEL_BYTES],
    /// Exact [`WORKER_GPU_RECEIPT_SCHEMA`] label.
    pub schema: [u8; WORKER_RECEIPT_LABEL_BYTES],
    /// Nonzero receipt sequence.
    pub sequence: u64,
    /// Full identity of the GPU Worker.
    pub identity: WorkerIdentity,
    /// One of the three GPU lease action codes.
    pub action: u16,
    /// Confirmed, rejected, or stale terminal outcome.
    pub outcome: u16,
    /// Reserved; must be zero.
    pub reserved: u32,
    /// Exact action and result digests.
    pub digests: ReceiptDigests,
    /// Sequence-last commit; must equal `sequence`.
    pub committed_sequence: u64,
}

impl GpuLeaseReceiptRecord {
    /// Empty, uncommitted GPU receipt.
    pub const EMPTY: Self = Self {
        magic: WORKER_GPU_RECEIPT_MAGIC,
        version: WORKER_TASK_ABI_VERSION,
        length: size_of::<Self>() as u16,
        label: GPU_LEASE_RECEIPT,
        schema: WORKER_GPU_RECEIPT_SCHEMA,
        sequence: 0,
        identity: WorkerControlRecord::EMPTY.identity,
        action: 0,
        outcome: 0,
        reserved: 0,
        digests: ReceiptDigests::EMPTY,
        committed_sequence: 0,
    };

    /// Construct a staged receipt from a validated GPU control record.
    pub fn staged(control: WorkerControlRecord) -> Result<Self, WorkerAbiError> {
        let action = control.worker_action()?;
        let outcome = control.worker_outcome()?;
        if !action.is_gpu() {
            return Err(WorkerAbiError::InvalidAction);
        }
        if !outcome.terminal() {
            return Err(WorkerAbiError::InvalidOutcome);
        }
        if !control.digests.complete() {
            return Err(WorkerAbiError::InvalidDigest);
        }
        Ok(Self {
            magic: WORKER_GPU_RECEIPT_MAGIC,
            version: WORKER_TASK_ABI_VERSION,
            length: size_of::<Self>() as u16,
            label: GPU_LEASE_RECEIPT,
            schema: WORKER_GPU_RECEIPT_SCHEMA,
            sequence: control.sequence,
            identity: control.identity,
            action: control.action,
            outcome: control.outcome,
            reserved: 0,
            digests: control.digests,
            committed_sequence: 0,
        })
    }

    /// Return the receipt with its commit sequence populated.
    #[must_use]
    pub const fn committed(mut self) -> Self {
        self.committed_sequence = self.sequence;
        self
    }

    /// Validate an exact stable GPU receipt.
    pub fn validate_for(self, init: WorkerRuntimeInit) -> Result<(), WorkerAbiError> {
        if self.magic != WORKER_GPU_RECEIPT_MAGIC {
            return Err(WorkerAbiError::InvalidMagic);
        }
        if self.version != WORKER_TASK_ABI_VERSION {
            return Err(WorkerAbiError::InvalidVersion);
        }
        if self.length as usize != size_of::<Self>() {
            return Err(WorkerAbiError::InvalidLength);
        }
        if self.label != GPU_LEASE_RECEIPT || self.schema != WORKER_GPU_RECEIPT_SCHEMA {
            return Err(WorkerAbiError::InvalidReceiptLabel);
        }
        if self.sequence == 0 || self.committed_sequence != self.sequence {
            return Err(WorkerAbiError::InvalidSequence);
        }
        if self.identity != init.identity || self.identity.role != WorkerRole::Gpu as u16 {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        let action = WorkerAction::from_raw(self.action)?;
        if !action.is_gpu() {
            return Err(WorkerAbiError::InvalidAction);
        }
        let outcome = WorkerOutcome::from_raw(self.outcome)?;
        if !outcome.terminal() {
            return Err(WorkerAbiError::InvalidOutcome);
        }
        if self.reserved != 0 || !self.digests.complete() {
            return Err(WorkerAbiError::InvalidDigest);
        }
        Ok(())
    }
}

/// Exact fixed-layout PEFT lifecycle receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PeftReceiptRecord {
    /// [`WORKER_PEFT_RECEIPT_MAGIC`].
    pub magic: u32,
    /// [`WORKER_TASK_ABI_VERSION`].
    pub version: u16,
    /// Exact record length.
    pub length: u16,
    /// Exact [`PEFT_RECEIPT`] label.
    pub label: [u8; WORKER_RECEIPT_LABEL_BYTES],
    /// Exact [`WORKER_LORA_RECEIPT_SCHEMA`] label.
    pub schema: [u8; WORKER_RECEIPT_LABEL_BYTES],
    /// Nonzero receipt sequence.
    pub sequence: u64,
    /// Full identity of the LoRA Worker.
    pub identity: WorkerIdentity,
    /// One of the four PEFT lifecycle action codes.
    pub action: u16,
    /// Confirmed, rejected, or stale terminal outcome.
    pub outcome: u16,
    /// Reserved; must be zero.
    pub reserved: u32,
    /// Exact action and result digests.
    pub digests: ReceiptDigests,
    /// Sequence-last commit; must equal `sequence`.
    pub committed_sequence: u64,
}

impl PeftReceiptRecord {
    /// Empty, uncommitted PEFT receipt.
    pub const EMPTY: Self = Self {
        magic: WORKER_PEFT_RECEIPT_MAGIC,
        version: WORKER_TASK_ABI_VERSION,
        length: size_of::<Self>() as u16,
        label: PEFT_RECEIPT,
        schema: WORKER_LORA_RECEIPT_SCHEMA,
        sequence: 0,
        identity: WorkerControlRecord::EMPTY.identity,
        action: 0,
        outcome: 0,
        reserved: 0,
        digests: ReceiptDigests::EMPTY,
        committed_sequence: 0,
    };

    /// Construct a staged receipt from a validated LoRA control record.
    pub fn staged(control: WorkerControlRecord) -> Result<Self, WorkerAbiError> {
        let action = control.worker_action()?;
        let outcome = control.worker_outcome()?;
        if !action.is_peft() {
            return Err(WorkerAbiError::InvalidAction);
        }
        if !outcome.terminal() {
            return Err(WorkerAbiError::InvalidOutcome);
        }
        if !control.digests.complete() {
            return Err(WorkerAbiError::InvalidDigest);
        }
        Ok(Self {
            magic: WORKER_PEFT_RECEIPT_MAGIC,
            version: WORKER_TASK_ABI_VERSION,
            length: size_of::<Self>() as u16,
            label: PEFT_RECEIPT,
            schema: WORKER_LORA_RECEIPT_SCHEMA,
            sequence: control.sequence,
            identity: control.identity,
            action: control.action,
            outcome: control.outcome,
            reserved: 0,
            digests: control.digests,
            committed_sequence: 0,
        })
    }

    /// Return the receipt with its commit sequence populated.
    #[must_use]
    pub const fn committed(mut self) -> Self {
        self.committed_sequence = self.sequence;
        self
    }

    /// Validate an exact stable PEFT receipt.
    pub fn validate_for(self, init: WorkerRuntimeInit) -> Result<(), WorkerAbiError> {
        if self.magic != WORKER_PEFT_RECEIPT_MAGIC {
            return Err(WorkerAbiError::InvalidMagic);
        }
        if self.version != WORKER_TASK_ABI_VERSION {
            return Err(WorkerAbiError::InvalidVersion);
        }
        if self.length as usize != size_of::<Self>() {
            return Err(WorkerAbiError::InvalidLength);
        }
        if self.label != PEFT_RECEIPT || self.schema != WORKER_LORA_RECEIPT_SCHEMA {
            return Err(WorkerAbiError::InvalidReceiptLabel);
        }
        if self.sequence == 0 || self.committed_sequence != self.sequence {
            return Err(WorkerAbiError::InvalidSequence);
        }
        if self.identity != init.identity || self.identity.role != WorkerRole::Lora as u16 {
            return Err(WorkerAbiError::InvalidIdentity);
        }
        let action = WorkerAction::from_raw(self.action)?;
        if !action.is_peft() {
            return Err(WorkerAbiError::InvalidAction);
        }
        let outcome = WorkerOutcome::from_raw(self.outcome)?;
        if !outcome.terminal() {
            return Err(WorkerAbiError::InvalidOutcome);
        }
        if self.reserved != 0 || !self.digests.complete() {
            return Err(WorkerAbiError::InvalidDigest);
        }
        Ok(())
    }
}

const WORKER_SHARED_RECORD_BYTES: usize = size_of::<WorkerRuntimeInit>()
    + size_of::<WorkerControlRecord>()
    + size_of::<WorkerCompletionRecord>()
    + size_of::<WorkerReadyRecord>()
    + size_of::<GpuLeaseReceiptRecord>()
    + size_of::<PeftReceiptRecord>();

/// One page shared by root and exactly one Worker instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C, align(4096))]
pub struct WorkerSharedPage {
    /// Immutable, sequence-last runtime init descriptor.
    pub init: WorkerRuntimeInit,
    /// Root-produced durable control record.
    pub control: WorkerControlRecord,
    /// Worker-produced durable completion record.
    pub completion: WorkerCompletionRecord,
    /// Worker-produced READY record.
    pub ready: WorkerReadyRecord,
    /// WorkerGpu-produced receipt record.
    pub gpu_receipt: GpuLeaseReceiptRecord,
    /// WorkerLora-produced receipt record.
    pub peft_receipt: PeftReceiptRecord,
    /// Reserved zero bytes completing the fixed one-page layout.
    pub reserved: [u8; WORKER_SHARED_PAGE_BYTES - WORKER_SHARED_RECORD_BYTES],
}

impl WorkerSharedPage {
    /// Construct a zeroed protocol page around a staged init descriptor.
    #[must_use]
    pub const fn empty(init: WorkerRuntimeInit) -> Self {
        Self {
            init,
            control: WorkerControlRecord::EMPTY,
            completion: WorkerCompletionRecord::EMPTY,
            ready: WorkerReadyRecord::EMPTY,
            gpu_receipt: GpuLeaseReceiptRecord::EMPTY,
            peft_receipt: PeftReceiptRecord::EMPTY,
            reserved: [0; WORKER_SHARED_PAGE_BYTES - WORKER_SHARED_RECORD_BYTES],
        }
    }
}

const _: () = {
    assert!(size_of::<WorkerImageMetadata>() == 64);
    assert!(align_of::<WorkerImageMetadata>() == 8);
    assert!(offset_of!(WorkerImageMetadata, magic) == 0);
    assert!(offset_of!(WorkerImageMetadata, abi_version) == 4);
    assert!(offset_of!(WorkerImageMetadata, metadata_length) == 6);
    assert!(offset_of!(WorkerImageMetadata, role) == 8);
    assert!(offset_of!(WorkerImageMetadata, entry_version) == 10);
    assert!(offset_of!(WorkerImageMetadata, flags) == 12);
    assert!(offset_of!(WorkerImageMetadata, entry_symbol) == 16);
    assert!(offset_of!(WorkerImageMetadata, reserved) == 48);
    assert!(size_of::<WorkerIdentity>() == 32);
    assert!(size_of::<Digest32>() == 32);
    assert!(size_of::<WorkerSharedPage>() == WORKER_SHARED_PAGE_BYTES);
    assert!(align_of::<WorkerSharedPage>() == WORKER_SHARED_PAGE_ALIGNMENT);
    assert!(
        offset_of!(WorkerRuntimeInit, committed_sequence) + size_of::<u64>()
            == size_of::<WorkerRuntimeInit>()
    );
    assert!(
        offset_of!(WorkerControlRecord, committed_sequence) + size_of::<u64>()
            == size_of::<WorkerControlRecord>()
    );
    assert!(
        offset_of!(WorkerCompletionRecord, committed_sequence) + size_of::<u64>()
            == size_of::<WorkerCompletionRecord>()
    );
    assert!(
        offset_of!(WorkerReadyRecord, committed_sequence) + size_of::<u64>()
            == size_of::<WorkerReadyRecord>()
    );
    assert!(
        offset_of!(GpuLeaseReceiptRecord, committed_sequence) + size_of::<u64>()
            == size_of::<GpuLeaseReceiptRecord>()
    );
    assert!(
        offset_of!(PeftReceiptRecord, committed_sequence) + size_of::<u64>()
            == size_of::<PeftReceiptRecord>()
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    const fn digest(seed: u8) -> Digest32 {
        Digest32::new([seed; 32])
    }

    const fn labels() -> WorkerCallLabels {
        WorkerCallLabels {
            control: 11,
            shutdown: 12,
            revoke: 13,
        }
    }

    const fn identity(role: WorkerRole) -> WorkerIdentity {
        WorkerIdentity::new(role, 2, 3, 4, 5)
    }

    const fn init(role: WorkerRole) -> WorkerRuntimeInit {
        WorkerRuntimeInit::staged(
            1,
            identity(role),
            0x7000_0000,
            1,
            2,
            3,
            labels(),
            0x2601,
            1 << 12,
            digest(0x11),
            digest(0x22),
        )
        .committed()
    }

    const fn receipt_digests() -> ReceiptDigests {
        ReceiptDigests {
            ticket: digest(1),
            idempotency: digest(2),
            operation: digest(3),
            subject: digest(4),
            result: digest(5),
        }
    }

    #[test]
    fn shared_page_and_sequence_last_layouts_are_fixed() {
        assert_eq!(size_of::<WorkerSharedPage>(), WORKER_SHARED_PAGE_BYTES);
        assert_eq!(align_of::<WorkerSharedPage>(), WORKER_SHARED_PAGE_ALIGNMENT);
        assert_eq!(size_of::<WorkerIdentity>(), 32);
        assert!(!core::mem::needs_drop::<WorkerSharedPage>());
        assert_eq!(
            offset_of!(WorkerControlRecord, committed_sequence) + size_of::<u64>(),
            size_of::<WorkerControlRecord>()
        );
        assert!(offset_of!(WorkerSharedPage, control) > offset_of!(WorkerSharedPage, init));
        assert!(offset_of!(WorkerSharedPage, completion) > offset_of!(WorkerSharedPage, control));
    }

    #[test]
    fn image_metadata_is_fixed_pointer_free_and_role_exact() {
        assert_eq!(WORKER_IMAGE_METADATA_SECTION, ".cohesix.worker");
        assert_eq!(size_of::<WorkerImageMetadata>(), 64);
        assert_eq!(align_of::<WorkerImageMetadata>(), 8);
        assert_eq!(offset_of!(WorkerImageMetadata, magic), 0);
        assert_eq!(offset_of!(WorkerImageMetadata, abi_version), 4);
        assert_eq!(offset_of!(WorkerImageMetadata, metadata_length), 6);
        assert_eq!(offset_of!(WorkerImageMetadata, role), 8);
        assert_eq!(offset_of!(WorkerImageMetadata, entry_version), 10);
        assert_eq!(offset_of!(WorkerImageMetadata, flags), 12);
        assert_eq!(offset_of!(WorkerImageMetadata, entry_symbol), 16);
        assert_eq!(offset_of!(WorkerImageMetadata, reserved), 48);
        assert!(!core::mem::needs_drop::<WorkerImageMetadata>());

        for role in [WorkerRole::Heartbeat, WorkerRole::Gpu, WorkerRole::Lora] {
            let metadata = WorkerImageMetadata::for_role(role);
            assert_eq!(metadata.validate(), Ok(role));
            assert_eq!(metadata.validate_for_role(role), Ok(()));
            assert_eq!(metadata.entry_symbol, fixed_entry_symbol(b"_start"));
        }

        let mut wrong_role = WorkerImageMetadata::for_role(WorkerRole::Heartbeat);
        wrong_role.role = 4;
        assert_eq!(wrong_role.validate(), Err(WorkerAbiError::InvalidRole));

        let mut wrong_symbol = WorkerImageMetadata::for_role(WorkerRole::Gpu);
        wrong_symbol.entry_symbol[0] = b'X';
        assert_eq!(
            wrong_symbol.validate(),
            Err(WorkerAbiError::InvalidEntrySymbol)
        );

        let gpu = WorkerImageMetadata::for_role(WorkerRole::Gpu);
        assert_eq!(
            gpu.validate_for_role(WorkerRole::Lora),
            Err(WorkerAbiError::InvalidRole)
        );
    }

    #[test]
    fn five_part_identity_rejects_missing_generations_and_model_only_roles() {
        assert_eq!(identity(WorkerRole::Heartbeat).validate(), Ok(()));
        let mut invalid = identity(WorkerRole::Gpu);
        invalid.supervisor_generation = 0;
        assert_eq!(invalid.validate(), Err(WorkerAbiError::InvalidIdentity));
        invalid = identity(WorkerRole::Gpu);
        invalid.role = 4;
        assert_eq!(invalid.validate(), Err(WorkerAbiError::InvalidIdentity));
    }

    #[test]
    fn init_requires_exact_commit_seal_slots_bits_and_role() {
        let valid = init(WorkerRole::Heartbeat);
        assert_eq!(valid.validate_for_role(WorkerRole::Heartbeat), Ok(()));
        assert_eq!(
            valid.validate_for_role(WorkerRole::Gpu),
            Err(WorkerAbiError::InvalidIdentity)
        );

        let mut torn = valid;
        torn.committed_sequence = 0;
        assert_eq!(
            torn.validate_for_role(WorkerRole::Heartbeat),
            Err(WorkerAbiError::InvalidSequence)
        );

        let mut tampered = valid;
        tampered.identity.cap_generation += 1;
        assert_eq!(
            tampered.validate_for_role(WorkerRole::Heartbeat),
            Err(WorkerAbiError::InvalidSeal)
        );

        let mut aliased = WorkerRuntimeInit::staged(
            1,
            identity(WorkerRole::Heartbeat),
            0x7000_0000,
            2,
            2,
            3,
            labels(),
            0x2601,
            1 << 12,
            digest(0x11),
            digest(0x22),
        )
        .committed();
        assert_eq!(
            aliased.validate_for_role(WorkerRole::Heartbeat),
            Err(WorkerAbiError::InvalidCapabilitySlot)
        );
        aliased.service_endpoint_slot = 1;
        aliased.seal = aliased.expected_seal();
        assert_eq!(aliased.validate_for_role(WorkerRole::Heartbeat), Ok(()));
    }

    #[test]
    fn call_labels_are_exact_distinct_and_non_coalescing() {
        let causes = labels();
        assert_eq!(causes.validate(), Ok(()));
        assert_eq!(
            causes.classify(causes.control),
            Ok(WorkerCallOperation::Control)
        );
        assert_eq!(
            causes.classify(causes.shutdown),
            Ok(WorkerCallOperation::Shutdown)
        );
        assert_eq!(
            causes.classify(causes.revoke),
            Ok(WorkerCallOperation::Revoke)
        );
        assert_eq!(causes.classify(1), Err(WorkerAbiError::InvalidCallLabel));
        let overlap = WorkerCallLabels {
            shutdown: causes.control,
            ..causes
        };
        assert_eq!(overlap.validate(), Err(WorkerAbiError::InvalidCallLabel));
    }

    #[test]
    fn control_matrix_is_role_and_digest_exact() {
        let heartbeat_init = init(WorkerRole::Heartbeat);
        let heartbeat = WorkerControlRecord::staged(
            1,
            heartbeat_init.identity,
            WorkerAction::HeartbeatPublish,
            WorkerOutcome::NotApplicable,
            10,
            20,
            ReceiptDigests::EMPTY,
        )
        .committed();
        assert_eq!(heartbeat.validate_for(heartbeat_init), Ok(()));

        let gpu_init = init(WorkerRole::Gpu);
        let gpu = WorkerControlRecord::staged(
            1,
            gpu_init.identity,
            WorkerAction::GpuLeaseGrant,
            WorkerOutcome::Confirmed,
            10,
            20,
            receipt_digests(),
        )
        .committed();
        assert_eq!(gpu.validate_for(gpu_init), Ok(()));

        let wrong_role = WorkerControlRecord {
            identity: heartbeat_init.identity,
            ..gpu
        };
        assert_eq!(
            wrong_role.validate_for(heartbeat_init),
            Err(WorkerAbiError::InvalidAction)
        );
        let missing_digest = WorkerControlRecord {
            digests: ReceiptDigests::EMPTY,
            ..gpu
        };
        assert_eq!(
            missing_digest.validate_for(gpu_init),
            Err(WorkerAbiError::InvalidDigest)
        );
    }

    #[test]
    fn ready_and_completion_copy_exact_init_and_control_identity() {
        let gpu_init = init(WorkerRole::Gpu);
        let ready = WorkerReadyRecord::staged(gpu_init, 1).committed();
        assert_eq!(ready.validate_for(gpu_init), Ok(()));

        let control = WorkerControlRecord::staged(
            7,
            gpu_init.identity,
            WorkerAction::GpuLeaseRenew,
            WorkerOutcome::Rejected,
            10,
            20,
            receipt_digests(),
        )
        .committed();
        let completion = WorkerCompletionRecord::staged_for_control(control).committed();
        assert_eq!(completion.status, WorkerCompletionStatus::Rejected as u16);
        assert_eq!(completion.result_digest, control.digests.result);
        assert_eq!(completion.validate_for(gpu_init), Ok(()));
    }

    #[test]
    fn stale_outcome_round_trips_without_aliasing_rejected() {
        assert_eq!(WorkerOutcome::from_raw(8), Ok(WorkerOutcome::Stale));
        assert_eq!(
            WorkerCompletionStatus::from_raw(8),
            Ok(WorkerCompletionStatus::Stale)
        );
        let gpu_init = init(WorkerRole::Gpu);
        let control = WorkerControlRecord::staged(
            8,
            gpu_init.identity,
            WorkerAction::GpuLeaseRelease,
            WorkerOutcome::Stale,
            10,
            20,
            receipt_digests(),
        )
        .committed();
        assert_eq!(control.validate_for(gpu_init), Ok(()));
        let receipt = GpuLeaseReceiptRecord::staged(control)
            .expect("stale GPU receipt")
            .committed();
        assert_eq!(receipt.outcome, WorkerOutcome::Stale as u16);
        assert_eq!(receipt.validate_for(gpu_init), Ok(()));
        let completion = WorkerCompletionRecord::staged_for_control(control).committed();
        assert_eq!(completion.status, WorkerCompletionStatus::Stale as u16);
        assert_eq!(completion.validate_for(gpu_init), Ok(()));

        let lora_init = init(WorkerRole::Lora);
        let peft_control = WorkerControlRecord::staged(
            9,
            lora_init.identity,
            WorkerAction::PeftExport,
            WorkerOutcome::Stale,
            10,
            20,
            receipt_digests(),
        )
        .committed();
        assert_eq!(peft_control.validate_for(lora_init), Ok(()));
        let peft_receipt = PeftReceiptRecord::staged(peft_control)
            .expect("stale PEFT receipt")
            .committed();
        assert_eq!(peft_receipt.outcome, WorkerOutcome::Stale as u16);
        assert_eq!(peft_receipt.validate_for(lora_init), Ok(()));
        let peft_completion = WorkerCompletionRecord::staged_for_control(peft_control).committed();
        assert_eq!(peft_completion.status, WorkerCompletionStatus::Stale as u16);
        assert_eq!(peft_completion.validate_for(lora_init), Ok(()));
    }

    #[test]
    fn gpu_receipt_accepts_exactly_three_gpu_actions() {
        let gpu_init = init(WorkerRole::Gpu);
        for action in [
            WorkerAction::GpuLeaseGrant,
            WorkerAction::GpuLeaseRenew,
            WorkerAction::GpuLeaseRelease,
        ] {
            let control = WorkerControlRecord::staged(
                action as u64,
                gpu_init.identity,
                action,
                WorkerOutcome::Confirmed,
                0,
                0,
                receipt_digests(),
            )
            .committed();
            let receipt = GpuLeaseReceiptRecord::staged(control)
                .expect("GPU action must construct")
                .committed();
            assert_eq!(receipt.validate_for(gpu_init), Ok(()));
            assert_eq!(receipt.label, GPU_LEASE_RECEIPT);
        }
        let peft_control = WorkerControlRecord::staged(
            9,
            gpu_init.identity,
            WorkerAction::PeftImport,
            WorkerOutcome::Confirmed,
            0,
            0,
            receipt_digests(),
        )
        .committed();
        assert_eq!(
            GpuLeaseReceiptRecord::staged(peft_control),
            Err(WorkerAbiError::InvalidAction)
        );
    }

    #[test]
    fn peft_receipt_accepts_exactly_four_peft_actions() {
        let lora_init = init(WorkerRole::Lora);
        for (sequence, action) in [
            WorkerAction::PeftExport,
            WorkerAction::PeftImport,
            WorkerAction::PeftActivate,
            WorkerAction::PeftRollback,
        ]
        .into_iter()
        .enumerate()
        {
            let control = WorkerControlRecord::staged(
                sequence as u64 + 1,
                lora_init.identity,
                action,
                WorkerOutcome::Rejected,
                0,
                0,
                receipt_digests(),
            )
            .committed();
            let receipt = PeftReceiptRecord::staged(control)
                .expect("PEFT action must construct")
                .committed();
            assert_eq!(receipt.validate_for(lora_init), Ok(()));
            assert_eq!(receipt.label, PEFT_RECEIPT);
        }
    }

    #[test]
    fn receipt_tampering_and_cross_generation_replay_fail_closed() {
        let gpu_init = init(WorkerRole::Gpu);
        let control = WorkerControlRecord::staged(
            2,
            gpu_init.identity,
            WorkerAction::GpuLeaseRelease,
            WorkerOutcome::Confirmed,
            0,
            0,
            receipt_digests(),
        )
        .committed();
        let receipt = GpuLeaseReceiptRecord::staged(control)
            .expect("valid receipt")
            .committed();

        let mut wrong_label = receipt;
        wrong_label.label[0] = b'X';
        assert_eq!(
            wrong_label.validate_for(gpu_init),
            Err(WorkerAbiError::InvalidReceiptLabel)
        );

        let mut next_generation = gpu_init;
        next_generation.identity.supervisor_generation += 1;
        next_generation.seal = next_generation.expected_seal();
        assert_eq!(
            receipt.validate_for(next_generation),
            Err(WorkerAbiError::InvalidIdentity)
        );

        let mut torn = receipt;
        torn.committed_sequence = 0;
        assert_eq!(
            torn.validate_for(gpu_init),
            Err(WorkerAbiError::InvalidSequence)
        );
    }
}
