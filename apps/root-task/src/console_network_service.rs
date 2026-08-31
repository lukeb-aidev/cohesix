// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Supervise the isolated console-network child from root policy authority.
// Author: Lukas Bower

//! Root-owned boundary for the isolated TCP console/network child.

use console_network_abi::{
    AbiError, CommandBatchCursor, ExchangeKind, ExchangePage, PacketDirection, PacketPage,
    RuntimeInitDescriptor, ABI_VERSION, AUTH_TOKEN_BYTES, CHILD_CSPACE_SLOTS, CHILD_WAKE_MASK,
    CHILD_WAKE_NOTIFICATION_SLOT, COMMAND_BATCH_MAX_RECORDS, COMMAND_LINE_BYTES,
    DIRECT_GENET_SHARED_PAGE_COUNT, ETHERNET_FRAME_BYTES, FAULT_ENDPOINT_SLOT,
    PACKET_TX_WAKE_NOTIFICATION_SLOT, REQUIRED_INIT_FLAGS, ROOT_CONTROL_WAKE_NOTIFICATION_SLOT,
    ROOT_WAKE_MASK, RUNTIME_INIT_MAGIC, SHARED_PAGE_BYTES, SUPERVISOR_WAKE_NOTIFICATION_SLOT,
    WAKE_CONTROL, WAKE_PUBLICATION_ACK,
};
use heapless::Vec as HeaplessVec;

/// Compiler-selected service task ID.
pub const SERVICE_TASK_ID: &str = "console-network-service";
/// Runtime READY identity accepted before packet or console admission.
pub const READY_IDENTITY: &str = "console-network-service/v6";
const FIXED_SERVICE_FRAME_COUNT: u32 = 38;
const RETAINED_ROOT_SLOT_OVERHEAD: u32 = 25;
const DIRECT_VIRTIO_DMA_FRAME_COUNT: u32 = 34;
const DIRECT_VIRTIO_RETAINED_SLOT_COUNT: u32 = 3;
const DIRECT_GENET_RETAINED_SLOT_COUNT: u32 = DIRECT_GENET_SHARED_PAGE_COUNT as u32;
mod generated_image_identity {
    ::core::include!(::core::concat!(
        ::core::env!("OUT_DIR"),
        "/console_network_image_identity.rs"
    ));
}

pub use generated_image_identity::{
    CONSOLE_NETWORK_IMAGE_IDENTITY_BOUND, CONSOLE_NETWORK_RUNTIME_BYTES,
    CONSOLE_NETWORK_RUNTIME_ENTRY_VADDR, CONSOLE_NETWORK_RUNTIME_IMAGE,
    CONSOLE_NETWORK_RUNTIME_LOAD_BASE_VADDR, CONSOLE_NETWORK_RUNTIME_LOAD_LIMIT_VADDR,
    CONSOLE_NETWORK_RUNTIME_LOAD_PAGES, CONSOLE_NETWORK_RUNTIME_SHA256,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConsoleNetworkSignalOnlyTopology {
    required: bool,
    root_core: u8,
    root_sched_control_core: u8,
    root_priority: u8,
    root_mcp: u8,
    child_core: u8,
    child_sched_control_core: u8,
    child_priority: u8,
    child_scheduling_context_slot: u32,
}

fn validate_console_network_signal_only_topology(
    topology: ConsoleNetworkSignalOnlyTopology,
) -> Result<(), BoundaryError> {
    if !topology.required {
        return Ok(());
    }
    if topology.child_scheduling_context_slot == 0
        || topology.root_core != 0
        || topology.child_core != 2
        || topology.root_core == topology.child_core
        || topology.root_sched_control_core != topology.root_core
        || topology.child_sched_control_core != topology.child_core
        || topology.root_priority != topology.child_priority
        || topology.root_mcp < topology.child_priority
    {
        return Err(BoundaryError::TemporalDrift);
    }
    Ok(())
}

fn select_console_network_cross_core_signal_only(
    direct_virtio: bool,
    direct_genet: bool,
) -> Result<bool, BoundaryError> {
    match (direct_virtio, direct_genet) {
        (true, false) | (false, false) => Ok(false),
        (false, true) => Ok(true),
        (true, true) => Err(BoundaryError::GeneratedDrift),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConsoleNetworkSignalOnlyAdmission {
    pub(crate) contract_direct_genet: bool,
    pub(crate) runtime_direct_genet: bool,
    pub(crate) cross_core_signal_only: bool,
    pub(crate) yield_to_child_after_signal: bool,
    pub(crate) activated: bool,
    pub(crate) containment_started: bool,
    pub(crate) contained: bool,
    pub(crate) scheduling_context_present: bool,
}

pub(crate) const fn console_network_signal_only_admitted(
    admission: ConsoleNetworkSignalOnlyAdmission,
) -> bool {
    !admission.yield_to_child_after_signal
        && (if admission.contract_direct_genet {
            admission.runtime_direct_genet
                && admission.cross_core_signal_only
                && admission.activated
                && !admission.containment_started
                && !admission.contained
                && admission.scheduling_context_present
        } else {
            !admission.runtime_direct_genet && !admission.cross_core_signal_only
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConsoleNetworkYieldToAdmission {
    pub(crate) profile_enabled: bool,
    pub(crate) runtime_direct_virtio: bool,
    pub(crate) runtime_direct_genet: bool,
    pub(crate) service_state: ServiceState,
    pub(crate) durable_publication: bool,
    pub(crate) signal_badge: u64,
    pub(crate) activated: bool,
    pub(crate) containment_started: bool,
    pub(crate) contained: bool,
    pub(crate) scheduling_context_present: bool,
}

pub(crate) const fn console_network_yield_to_admitted(
    admission: ConsoleNetworkYieldToAdmission,
) -> bool {
    admission.profile_enabled
        && !admission.runtime_direct_virtio
        && admission.runtime_direct_genet
        && matches!(admission.service_state, ServiceState::Authenticated)
        && admission.durable_publication
        && admission.signal_badge.is_power_of_two()
        && matches!(admission.signal_badge, WAKE_CONTROL | WAKE_PUBLICATION_ACK)
        && admission.activated
        && !admission.containment_started
        && !admission.contained
        && admission.scheduling_context_present
}

/// Return the compiler-owned console-network service record.
#[must_use]
pub const fn generated_config() -> crate::generated::ConsoleNetworkServiceConfig {
    crate::generated::console_network_service_config()
}

/// Validated subset of generated truth consumed by construction and service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleNetworkContract {
    /// Child image ID.
    pub image_id: &'static str,
    /// Target package path in the system archive.
    pub image_path: &'static str,
    /// Exact ELF entry symbol.
    pub entry_symbol: &'static str,
    /// Sole TCP listener port.
    pub listener_port: u16,
    /// Child CSpace cardinality.
    pub child_cspace_slots: u16,
    /// Retained init-CNode slot containing the child-untyped revoke anchor.
    pub revoke_anchor_slot: u32,
    /// Size of the dedicated child untyped.
    pub revoke_anchor_bits: u8,
    /// Exact object and memory inventory retyped below the retained anchor.
    pub objects: crate::generated::KernelObjectBudget,
    /// Whether the isolated QEMU child owns the admitted VirtIO data path.
    pub direct_virtio: bool,
    /// Whether the Pi child exchanges packets directly with the isolated GENET owner.
    pub direct_genet: bool,
    /// Child wait notification slot.
    pub child_wake_slot: u32,
    /// Child packet-TX signal slot.
    pub packet_tx_wake_slot: u32,
    /// Child console-event signal slot.
    pub supervisor_wake_slot: u32,
    /// Standard fault endpoint slot.
    pub fault_endpoint_slot: u32,
    /// Child IPC-buffer address.
    pub ipc_buffer_vaddr: u64,
    /// Read-only sealed runtime-init descriptor mapping.
    pub init_vaddr: u64,
    /// Bottom of the child stack mapping.
    pub stack_vaddr: u64,
    /// Exact stack page count.
    pub stack_pages: u16,
    /// Child ingress packet page address.
    pub packet_rx_vaddr: u64,
    /// Child egress packet page address.
    pub packet_tx_vaddr: u64,
    /// Child root-control page address.
    pub command_vaddr: u64,
    /// Child event page address.
    pub event_vaddr: u64,
    /// Active service core.
    pub core: u8,
    /// Active scheduling-context slot identity.
    pub scheduling_context_slot: u32,
    /// Scheduling-context object bits.
    pub scheduling_context_bits: u8,
    /// Child priority.
    pub priority: u8,
    /// Child maximum controlled priority.
    pub mcp: u8,
    /// MCS budget in microseconds.
    pub budget_us: u32,
    /// MCS period in microseconds.
    pub period_us: u32,
    /// Total seL4 replenishment bound.
    pub max_refills: u8,
    /// Generated kernel action when the active SC exhausts its budget.
    pub timeout_policy: crate::generated::TimeoutPolicy,
    /// Compiler-owned timeout fault badge.
    pub timeout_badge: u64,
    /// Compiler-owned standard fault badge.
    pub standard_fault_badge: u64,
    /// Whether the selected Pi profile admits guarded YieldTo on this exact child SC.
    pub yield_to_child_after_signal: bool,
    /// Whether the selected Pi direct-GENET profile requires cross-core signal-only handoff.
    ///
    /// This remains explicit so retained-continuation policy cannot confuse
    /// same-core YieldTo with the superseded cross-core signal-only profile.
    pub(crate) cross_core_signal_only: bool,
    /// Selected virtual-counter frequency.
    pub timer_clock_hz: u64,
    /// Transport-authentication deadline.
    pub auth_timeout_ms: u32,
    /// Authenticated idle deadline.
    pub idle_timeout_ms: u32,
}

impl ConsoleNetworkContract {
    /// Validate generated object and temporal records as one construction unit.
    pub fn from_generated() -> Result<Self, BoundaryError> {
        let config = generated_config();
        let (expected_object_frames, expected_object_cspace_slots) =
            expected_object_inventory(config.direct_virtio, config.direct_genet)?;
        if !config.enabled
            || config.abi_version != ABI_VERSION
            || config.image_id != "console-network-runtime"
            || config.image_path != "cohesix/artifacts/console-network-runtime"
            || config.entry_symbol != "_start"
            || config.listener_port != cohesix_net_constants::COHESIX_TCP_CONSOLE_PORT
            || !config.single_listener
            || (cfg!(target_os = "none")
                && config.direct_virtio != cfg!(feature = "net-backend-virtio"))
            || (cfg!(target_os = "none")
                && config.direct_genet != cfg!(feature = "net-backend-genet-direct"))
            || (config.direct_virtio && config.direct_genet)
            || config.child_cspace_slots != CHILD_CSPACE_SLOTS as u16
            || COMMAND_LINE_BYTES != cohsh_core::MAX_LINE_LEN
            || config.revoke_anchor_slot != 16_136
            || config.revoke_anchor_bits != 20
            || config.objects.tcbs != 1
            || config.objects.cnodes != 1
            || config.objects.vspaces != 1
            || config.objects.page_tables != 8
            || config.objects.asids != 1
            || config.objects.frames != expected_object_frames
            || config.objects.endpoints != 0
            || config.objects.notifications != 2
            || config.objects.fault_caps != 1
            || config.objects.timeout_fault_caps != 1
            || config.objects.reply_objects != 0
            || config.objects.scheduling_contexts != 1
            || config.objects.cspace_slots != expected_object_cspace_slots
            || config.objects.untyped_bytes != 1_048_576
            || config.packet_rx_notification_slot != CHILD_WAKE_NOTIFICATION_SLOT
            || config.packet_tx_wake_notification_slot != PACKET_TX_WAKE_NOTIFICATION_SLOT
            || config.supervisor_wake_notification_slot != SUPERVISOR_WAKE_NOTIFICATION_SLOT
            || config.fault_endpoint_slot != FAULT_ENDPOINT_SLOT
            || config.stack_vaddr != 0x7203_0000
            || config.stack_pages != 32
            || config
                .stack_vaddr
                .checked_add(u64::from(config.stack_pages) * SHARED_PAGE_BYTES as u64)
                != Some(0x7205_0000)
            || config.shared_frame_bytes as usize != SHARED_PAGE_BYTES
            || config.ethernet_frame_bytes as usize != ETHERNET_FRAME_BYTES
            || config.max_packets_per_wake == 0
            || config.max_packets_per_wake > 16
            || config.max_commands_per_wake == 0
            || config.max_commands_per_wake as usize > COMMAND_BATCH_MAX_RECORDS
            || config.max_control_inflight != 1
            || config.packet_rx_badge != console_network_abi::WAKE_PACKET_RX
            || config.control_badge != console_network_abi::WAKE_CONTROL
            || config.shutdown_badge != console_network_abi::WAKE_SHUTDOWN
            || config.revoke_badge != console_network_abi::WAKE_REVOKE
            || config.packet_tx_ready_badge != console_network_abi::WAKE_PACKET_TX_READY
            || config.event_ready_badge != console_network_abi::WAKE_EVENT_READY
            || config.publication_ack_badge != console_network_abi::WAKE_PUBLICATION_ACK
            || config.fault_badge == 0
            || config.timer_clock_hz == 0
        {
            return Err(BoundaryError::GeneratedDrift);
        }
        let temporal = crate::generated::temporal_tasks()
            .iter()
            .find(|task| task.id == SERVICE_TASK_ID)
            .ok_or(BoundaryError::GeneratedDrift)?;
        if temporal.execution != crate::generated::TemporalExecution::Active
            || temporal.kind != crate::generated::TemporalTaskKind::Service
            || temporal.core != config.core
            || temporal.sched_control_core != config.core
            || temporal.scheduling_context_slot != config.scheduling_context_slot
            || temporal.scheduling_context_bits != config.scheduling_context_bits
            || temporal.priority != config.priority
            || temporal.mcp != config.mcp
            || temporal.budget_us != config.budget_us
            || temporal.period_us != config.period_us
            || temporal.max_refills != config.max_refills
            || temporal.timeout_policy != crate::generated::TimeoutPolicy::NaturalPostpone
            || temporal.timeout_badge != config.timeout_badge
            || !temporal.allowed_donors.is_empty()
            || temporal.reply_objects != 0
            || temporal.max_donation_depth != 0
        {
            return Err(BoundaryError::TemporalDrift);
        }
        let cross_core_signal_only = select_console_network_cross_core_signal_only(
            config.direct_virtio,
            config.direct_genet,
        )?;
        if cross_core_signal_only {
            let root_control = crate::generated::temporal_tasks()
                .iter()
                .find(|task| task.id == "root-control")
                .ok_or(BoundaryError::TemporalDrift)?;
            if root_control.kind != crate::generated::TemporalTaskKind::RootControl
                || root_control.execution != crate::generated::TemporalExecution::Active
            {
                return Err(BoundaryError::TemporalDrift);
            }
            validate_console_network_signal_only_topology(ConsoleNetworkSignalOnlyTopology {
                required: cross_core_signal_only,
                root_core: root_control.core,
                root_sched_control_core: root_control.sched_control_core,
                root_priority: root_control.priority,
                root_mcp: root_control.mcp,
                child_core: temporal.core,
                child_sched_control_core: temporal.sched_control_core,
                child_priority: temporal.priority,
                child_scheduling_context_slot: temporal.scheduling_context_slot,
            })?;
        }
        Ok(Self {
            image_id: config.image_id,
            image_path: config.image_path,
            entry_symbol: config.entry_symbol,
            listener_port: config.listener_port,
            child_cspace_slots: config.child_cspace_slots,
            revoke_anchor_slot: config.revoke_anchor_slot,
            revoke_anchor_bits: config.revoke_anchor_bits,
            objects: config.objects,
            direct_virtio: config.direct_virtio,
            direct_genet: config.direct_genet,
            child_wake_slot: config.packet_rx_notification_slot,
            packet_tx_wake_slot: config.packet_tx_wake_notification_slot,
            supervisor_wake_slot: config.supervisor_wake_notification_slot,
            fault_endpoint_slot: config.fault_endpoint_slot,
            ipc_buffer_vaddr: config.ipc_buffer_vaddr,
            init_vaddr: config.init_vaddr,
            stack_vaddr: config.stack_vaddr,
            stack_pages: config.stack_pages,
            packet_rx_vaddr: config.packet_rx_vaddr,
            packet_tx_vaddr: config.packet_tx_vaddr,
            command_vaddr: config.command_vaddr,
            event_vaddr: config.event_vaddr,
            core: config.core,
            scheduling_context_slot: config.scheduling_context_slot,
            scheduling_context_bits: config.scheduling_context_bits,
            priority: config.priority,
            mcp: config.mcp,
            budget_us: config.budget_us,
            period_us: config.period_us,
            max_refills: config.max_refills,
            timeout_policy: temporal.timeout_policy,
            timeout_badge: config.timeout_badge,
            standard_fault_badge: config.fault_badge,
            yield_to_child_after_signal: false,
            cross_core_signal_only,
            timer_clock_hz: config.timer_clock_hz,
            auth_timeout_ms: config.auth_timeout_ms,
            idle_timeout_ms: config.idle_timeout_ms,
        })
    }

    /// Build the sealed descriptor installed before the child is resumed.
    pub fn runtime_init(
        self,
        generation: u64,
        mac: [u8; 6],
        ipv4: [u8; 4],
        prefix_len: u8,
        gateway: [u8; 4],
        auth_token: &str,
    ) -> Result<RuntimeInitDescriptor, BoundaryError> {
        if generation == 0
            || auth_token.is_empty()
            || auth_token.len() > AUTH_TOKEN_BYTES
            || prefix_len == 0
            || prefix_len > 32
            || ipv4 == [0; 4]
        {
            return Err(BoundaryError::InvalidInput);
        }
        let mut token = [0; AUTH_TOKEN_BYTES];
        token[..auth_token.len()].copy_from_slice(auth_token.as_bytes());
        let descriptor = RuntimeInitDescriptor {
            magic: RUNTIME_INIT_MAGIC,
            abi_version: ABI_VERSION,
            descriptor_bytes: core::mem::size_of::<RuntimeInitDescriptor>() as u16,
            flags: REQUIRED_INIT_FLAGS,
            root_control_wake_notification_slot: ROOT_CONTROL_WAKE_NOTIFICATION_SLOT,
            generation,
            child_wake_notification_slot: self.child_wake_slot,
            packet_tx_wake_notification_slot: self.packet_tx_wake_slot,
            supervisor_wake_notification_slot: self.supervisor_wake_slot,
            fault_endpoint_slot: self.fault_endpoint_slot,
            child_cspace_slots: u32::from(self.child_cspace_slots),
            root_wake_mask: ROOT_WAKE_MASK,
            child_wake_mask: CHILD_WAKE_MASK,
            ipc_buffer_vaddr: self.ipc_buffer_vaddr,
            packet_rx_vaddr: self.packet_rx_vaddr,
            packet_tx_vaddr: self.packet_tx_vaddr,
            command_vaddr: self.command_vaddr,
            event_vaddr: self.event_vaddr,
            shared_frame_bytes: SHARED_PAGE_BYTES as u32,
            ethernet_frame_bytes: ETHERNET_FRAME_BYTES as u16,
            listener_port: self.listener_port,
            max_packets_per_wake: generated_config().max_packets_per_wake,
            max_commands_per_wake: generated_config().max_commands_per_wake,
            max_control_inflight: 1,
            prefix_len,
            auth_token_len: auth_token.len() as u8,
            reserved1: 0,
            mac,
            ipv4,
            gateway,
            reserved2: [0; 2],
            auth_timeout_ms: self.auth_timeout_ms,
            idle_timeout_ms: self.idle_timeout_ms,
            timer_clock_hz: self.timer_clock_hz,
            auth_token: token,
            seal: 0,
        }
        .sealed();
        descriptor
            .validate()
            .map_err(|_| BoundaryError::InvalidInit)?;
        Ok(descriptor)
    }
}

/// Exact runtime image-page inventory selected by the root constructor.
#[must_use]
pub(crate) const fn expected_runtime_image_pages(
    direct_virtio: bool,
    direct_genet: bool,
) -> Option<u16> {
    match (direct_virtio, direct_genet) {
        (true, false) => Some(62),
        (false, true) => Some(66),
        (false, false) => Some(60),
        (true, true) => None,
    }
}

fn expected_object_inventory(
    direct_virtio: bool,
    direct_genet: bool,
) -> Result<(u32, u32), BoundaryError> {
    let image_pages = expected_runtime_image_pages(direct_virtio, direct_genet)
        .ok_or(BoundaryError::GeneratedDrift)?;
    let direct_frames = if direct_virtio {
        DIRECT_VIRTIO_DMA_FRAME_COUNT
    } else {
        0
    };
    let retained_transport_slots = if direct_virtio {
        DIRECT_VIRTIO_RETAINED_SLOT_COUNT
    } else if direct_genet {
        DIRECT_GENET_RETAINED_SLOT_COUNT
    } else {
        0
    };
    let frames = u32::from(image_pages)
        .checked_add(FIXED_SERVICE_FRAME_COUNT)
        .and_then(|count| count.checked_add(direct_frames))
        .ok_or(BoundaryError::GeneratedDrift)?;
    let cspace_slots = frames
        .checked_add(RETAINED_ROOT_SLOT_OVERHEAD)
        .and_then(|count| count.checked_add(retained_transport_slots))
        .ok_or(BoundaryError::GeneratedDrift)?;
    Ok((frames, cspace_slots))
}

/// Exact fixed-object footprint reserved before constructing the child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleNetworkObjectPlan {
    /// Exact retained init-CNode slot.
    pub revoke_anchor_slot: u32,
    /// Exact child-untyped size.
    pub revoke_anchor_bits: u8,
    /// Complete compiler-owned kernel-object inventory.
    pub objects: crate::generated::KernelObjectBudget,
    /// ELF pages remaining after stack, IPC, init, and transport frames.
    pub image_pages: u16,
}

impl ConsoleNetworkObjectPlan {
    /// Derive the exact fixed footprint from compiler truth.
    pub fn from_generated() -> Result<Self, BoundaryError> {
        let contract = ConsoleNetworkContract::from_generated()?;
        let fixed_frames = u32::from(contract.stack_pages)
            .saturating_add(6)
            .saturating_add(if contract.direct_virtio { 34 } else { 0 });
        let image_pages = contract
            .objects
            .frames
            .checked_sub(fixed_frames)
            .and_then(|pages| u16::try_from(pages).ok())
            .ok_or(BoundaryError::GeneratedDrift)?;
        if image_pages == 0 {
            return Err(BoundaryError::GeneratedDrift);
        }
        Ok(Self {
            revoke_anchor_slot: contract.revoke_anchor_slot,
            revoke_anchor_bits: contract.revoke_anchor_bits,
            objects: contract.objects,
            image_pages,
        })
    }
}

/// Root-side lifecycle of one exact child generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    /// Objects exist but the child has not published READY.
    Constructing,
    /// READY is durable; exactly one listener is waiting.
    Listening,
    /// TCP accepted, but authentication is not complete.
    Authenticating,
    /// Transport authentication completed; root may execute typed commands.
    Authenticated,
    /// No new packet/control work is accepted while shutdown drains.
    Closing,
    /// A timeout or child fault requires supervisor containment.
    Faulted,
    /// A terminal child record stopped admission; kernel containment follows.
    Terminal,
}

/// Root-boundary failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundaryError {
    /// Generated constants disagree with ABI truth.
    GeneratedDrift,
    /// Generated temporal and object records disagree.
    TemporalDrift,
    /// A committed Pi child wake could not transfer execution to its exact SC.
    HandoffFailed,
    /// Dynamic network/authentication input is invalid.
    InvalidInput,
    /// Runtime-init validation failed.
    InvalidInit,
    /// A shared-page record is malformed.
    InvalidRecord,
    /// The record is stale, replayed, or acknowledges the wrong input.
    StaleIdentity,
    /// The current lifecycle cannot admit the requested transition.
    InvalidState,
    /// One bounded packet or control is already in flight.
    Backpressure,
    /// Child containment evidence is incomplete.
    IncompleteContainment,
}

impl From<AbiError> for BoundaryError {
    fn from(value: AbiError) -> Self {
        match value {
            AbiError::StaleGeneration | AbiError::InvalidSequence => Self::StaleIdentity,
            _ => Self::InvalidRecord,
        }
    }
}

/// One validated child event copied out of the untrusted shared page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleNetworkEvent {
    kind: ExchangeKind,
    connection_id: u64,
    now_ms: u64,
    related_sequence: u64,
    payload: HeaplessVec<u8, { console_network_abi::CONSOLE_PAYLOAD_BYTES }>,
}

impl ConsoleNetworkEvent {
    /// Typed event kind.
    #[must_use]
    pub const fn kind(&self) -> ExchangeKind {
        self.kind
    }

    /// Exact child connection identity.
    #[must_use]
    pub const fn connection_id(&self) -> u64 {
        self.connection_id
    }

    /// Child observation time.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Exact packet/control sequence acknowledged by a completion event.
    #[must_use]
    pub const fn related_sequence(&self) -> u64 {
        self.related_sequence
    }

    /// Exact initialized payload bytes.
    #[must_use]
    pub fn payload_bytes(&self) -> &[u8] {
        self.payload.as_slice()
    }

    /// Validated UTF-8 payload when the event kind carries plain text.
    pub fn payload(&self) -> Result<&str, BoundaryError> {
        core::str::from_utf8(self.payload.as_slice()).map_err(|_| BoundaryError::InvalidRecord)
    }
}

/// Proof required before any old-generation cap slot may be reused.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConsoleNetworkContainmentProof {
    /// Child TCB was suspended.
    pub tcb_suspended: bool,
    /// Active scheduling context was unbound.
    pub scheduling_context_unbound: bool,
    /// Four shared frames were scrubbed, cleaned, and unmapped before revoke.
    pub mappings_scrubbed: bool,
    /// Root and child notification/fault caps were revoked.
    pub capabilities_revoked: bool,
    /// TCB, CNode, VSpace, SC, frames, and translation objects were deleted.
    pub objects_deleted: bool,
    /// Next generation was durably fenced before reuse.
    pub generation_fenced: bool,
}

impl ConsoleNetworkContainmentProof {
    /// Whether complete teardown has been proven.
    #[must_use]
    pub const fn complete(self) -> bool {
        self.tcb_suspended
            && self.scheduling_context_unbound
            && self.mappings_scrubbed
            && self.capabilities_revoked
            && self.objects_deleted
            && self.generation_fenced
    }
}

/// One material unit in the exact console-network containment order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleNetworkContainmentUnit {
    /// Suspend the faulted child TCB.
    SuspendTcb,
    /// Unbind the child's scheduling context.
    UnbindSchedulingContext,
    /// Zero and clean one indexed shared frame.
    ScrubCleanSharedFrame(usize),
    /// Unmap one indexed shared frame after its clean completed.
    UnmapSharedFrame(usize),
    /// Suspend the paired GENET owner and delete both reciprocal signal caps.
    FenceDirectGenetPeer,
    /// Unmap one CPU-only direct-GENET frame copy from the console child.
    UnmapDirectGenetFrame(usize),
    /// Delete one root-held mapping cap after its direct-GENET unmap completed.
    DeleteDirectGenetFrameCap(usize),
    /// Clear the direct QEMU IRQHandler notification binding.
    ClearDirectIrq,
    /// Revoke the child-held direct QEMU IRQHandler copy.
    RevokeDirectIrqHandler,
    /// Delete the root's direct QEMU IRQ notification cap.
    DeleteDirectIrqNotification,
    /// Delete the root's direct QEMU IRQHandler cap.
    DeleteDirectIrqHandler,
    /// Reset, unmap, and delete the directly admitted QEMU device page.
    ResetUnmapDirectDevice,
    /// Unmap, scrub, clean, and release one direct-device DMA frame.
    ScrubDirectFrame(usize),
    /// Delete one indexed standard/timeout fault cap.
    DeleteFaultCap(usize),
    /// Revoke the generation anchor and reset its VSpace tracker.
    RevokeAnchor,
    /// Publish the exact proof and quarantine the terminal generation.
    Finalize,
    /// No containment units remain.
    Complete,
}

/// Pure progress cursor shared by the HAL implementation and focused tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleNetworkContainmentCursor {
    unit: ConsoleNetworkContainmentUnit,
    direct_frame_count: u8,
    direct_genet_frame_count: u8,
}

impl Default for ConsoleNetworkContainmentCursor {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleNetworkContainmentCursor {
    /// Exact number of shared-frame units before either fault cap is deleted.
    pub const SHARED_FRAME_COUNT: usize = 4;
    /// Exact number of fault-cap units before the retained anchor is revoked.
    pub const FAULT_CAP_COUNT: usize = 2;

    /// Start at the mandatory TCB suspension unit.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            unit: ConsoleNetworkContainmentUnit::SuspendTcb,
            direct_frame_count: 0,
            direct_genet_frame_count: 0,
        }
    }

    /// Start with an exact bounded direct-device DMA-frame inventory.
    #[must_use]
    pub const fn with_direct_frames(frame_count: u8) -> Self {
        Self {
            unit: ConsoleNetworkContainmentUnit::SuspendTcb,
            direct_frame_count: frame_count,
            direct_genet_frame_count: 0,
        }
    }

    /// Start with exact, mutually exclusive direct-device frame inventories.
    #[must_use]
    pub const fn with_direct_frame_inventories(
        direct_virtio_frame_count: u8,
        direct_genet_frame_count: u8,
    ) -> Self {
        Self {
            unit: ConsoleNetworkContainmentUnit::SuspendTcb,
            direct_frame_count: direct_virtio_frame_count,
            direct_genet_frame_count,
        }
    }

    /// Unit owned by the current exclusive recovery turn.
    #[must_use]
    pub const fn unit(self) -> ConsoleNetworkContainmentUnit {
        self.unit
    }

    /// Select one unit after durably committing its successor.
    pub fn select_next(&mut self) -> ConsoleNetworkContainmentUnit {
        let selected = self.unit;
        self.unit = match selected {
            ConsoleNetworkContainmentUnit::SuspendTcb => {
                ConsoleNetworkContainmentUnit::UnbindSchedulingContext
            }
            ConsoleNetworkContainmentUnit::UnbindSchedulingContext => {
                ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(0)
            }
            ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(frame_index) => {
                ConsoleNetworkContainmentUnit::UnmapSharedFrame(frame_index)
            }
            ConsoleNetworkContainmentUnit::UnmapSharedFrame(frame_index)
                if frame_index + 1 < Self::SHARED_FRAME_COUNT =>
            {
                ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(frame_index + 1)
            }
            ConsoleNetworkContainmentUnit::UnmapSharedFrame(_) if self.direct_frame_count != 0 => {
                ConsoleNetworkContainmentUnit::ClearDirectIrq
            }
            ConsoleNetworkContainmentUnit::UnmapSharedFrame(_)
                if self.direct_genet_frame_count != 0 =>
            {
                ConsoleNetworkContainmentUnit::FenceDirectGenetPeer
            }
            ConsoleNetworkContainmentUnit::UnmapSharedFrame(_) => {
                ConsoleNetworkContainmentUnit::DeleteFaultCap(0)
            }
            ConsoleNetworkContainmentUnit::FenceDirectGenetPeer => {
                ConsoleNetworkContainmentUnit::UnmapDirectGenetFrame(0)
            }
            ConsoleNetworkContainmentUnit::UnmapDirectGenetFrame(frame_index) => {
                ConsoleNetworkContainmentUnit::DeleteDirectGenetFrameCap(frame_index)
            }
            ConsoleNetworkContainmentUnit::DeleteDirectGenetFrameCap(frame_index)
                if frame_index + 1 < self.direct_genet_frame_count as usize =>
            {
                ConsoleNetworkContainmentUnit::UnmapDirectGenetFrame(frame_index + 1)
            }
            ConsoleNetworkContainmentUnit::DeleteDirectGenetFrameCap(_) => {
                ConsoleNetworkContainmentUnit::DeleteFaultCap(0)
            }
            ConsoleNetworkContainmentUnit::ClearDirectIrq => {
                ConsoleNetworkContainmentUnit::RevokeDirectIrqHandler
            }
            ConsoleNetworkContainmentUnit::RevokeDirectIrqHandler => {
                ConsoleNetworkContainmentUnit::DeleteDirectIrqNotification
            }
            ConsoleNetworkContainmentUnit::DeleteDirectIrqNotification => {
                ConsoleNetworkContainmentUnit::DeleteDirectIrqHandler
            }
            ConsoleNetworkContainmentUnit::DeleteDirectIrqHandler => {
                ConsoleNetworkContainmentUnit::ResetUnmapDirectDevice
            }
            ConsoleNetworkContainmentUnit::ResetUnmapDirectDevice => {
                ConsoleNetworkContainmentUnit::ScrubDirectFrame(0)
            }
            ConsoleNetworkContainmentUnit::ScrubDirectFrame(frame_index)
                if frame_index + 1 < self.direct_frame_count as usize =>
            {
                ConsoleNetworkContainmentUnit::ScrubDirectFrame(frame_index + 1)
            }
            ConsoleNetworkContainmentUnit::ScrubDirectFrame(_) => {
                ConsoleNetworkContainmentUnit::DeleteFaultCap(0)
            }
            ConsoleNetworkContainmentUnit::DeleteFaultCap(cap_index)
                if cap_index + 1 < Self::FAULT_CAP_COUNT =>
            {
                ConsoleNetworkContainmentUnit::DeleteFaultCap(cap_index + 1)
            }
            ConsoleNetworkContainmentUnit::DeleteFaultCap(_) => {
                ConsoleNetworkContainmentUnit::RevokeAnchor
            }
            ConsoleNetworkContainmentUnit::RevokeAnchor => ConsoleNetworkContainmentUnit::Finalize,
            ConsoleNetworkContainmentUnit::Finalize => ConsoleNetworkContainmentUnit::Complete,
            ConsoleNetworkContainmentUnit::Complete => ConsoleNetworkContainmentUnit::Complete,
        };
        selected
    }

    /// Restore a selected unit after its synchronous material action failed.
    pub fn restore_selected(&mut self, selected: ConsoleNetworkContainmentUnit) {
        self.unit = selected;
    }
}

/// Outcome of one exclusive root-control containment turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleNetworkContainmentTurn {
    /// No console-network fault or containment work is pending.
    Idle,
    /// Mailbox ownership was contended before a fault was latched; retry only.
    Retry,
    /// Exactly one containment unit completed; another recovery turn is required.
    InProgress,
    /// The exact complete proof is available and root may quarantine the service.
    Complete(ConsoleNetworkContainmentProof),
}

/// Exact newly consumed root inputs observed from one stable watermark pair.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConsoleNetworkInputCompletions {
    /// Ingress packet sequence newly consumed by the child.
    pub packet_sequence: Option<u64>,
    /// Root-control sequence newly consumed by the child.
    pub control_sequence: Option<u64>,
}

/// Exact root-to-child control publication whose consumption watermark is
/// still owed by the isolated child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleNetworkControlPublication {
    /// Compiler-owned isolated-child generation.
    pub generation: u64,
    /// Child-created connection that owns the root control record.
    pub connection_id: u64,
    /// Nonzero one-slot control sequence awaiting child consumption.
    pub sequence: u64,
}

impl ConsoleNetworkInputCompletions {
    /// Whether either independently bounded input made progress.
    #[must_use]
    pub const fn any(self) -> bool {
        self.packet_sequence.is_some() || self.control_sequence.is_some()
    }
}

/// Transactional root-side boundary for one isolated child generation.
pub struct ConsoleNetworkBoundary {
    contract: ConsoleNetworkContract,
    generation: u64,
    state: ServiceState,
    next_packet_sequence: u64,
    next_control_sequence: u64,
    last_event_sequence: u64,
    last_egress_sequence: u64,
    last_packet_consumed_sequence: u64,
    last_control_consumed_sequence: u64,
    packet_inflight: Option<u64>,
    control_inflight: Option<u64>,
    connection_id: Option<u64>,
    last_control_issued: u64,
    last_control_connection_id: Option<u64>,
    last_output_drained_sequence: u64,
    last_output_drained_connection_id: Option<u64>,
}

impl ConsoleNetworkBoundary {
    /// Begin construction of one nonzero generation.
    pub fn new(generation: u64) -> Result<Self, BoundaryError> {
        if generation == 0 {
            return Err(BoundaryError::InvalidInput);
        }
        Ok(Self {
            contract: ConsoleNetworkContract::from_generated()?,
            generation,
            state: ServiceState::Constructing,
            next_packet_sequence: 1,
            next_control_sequence: 1,
            last_event_sequence: 0,
            last_egress_sequence: 0,
            last_packet_consumed_sequence: 0,
            last_control_consumed_sequence: 0,
            packet_inflight: None,
            control_inflight: None,
            connection_id: None,
            last_control_issued: 0,
            last_control_connection_id: None,
            last_output_drained_sequence: 0,
            last_output_drained_connection_id: None,
        })
    }

    /// Compiler-validated construction contract.
    #[must_use]
    pub const fn contract(&self) -> ConsoleNetworkContract {
        self.contract
    }

    /// Current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> ServiceState {
        self.state
    }

    /// Exact current generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether the root may execute a child-delivered command.
    #[must_use]
    pub const fn authenticated_connection(&self) -> Option<u64> {
        if matches!(self.state, ServiceState::Authenticated) {
            self.connection_id
        } else {
            None
        }
    }

    /// Whether the one-slot packet ingress record may be published.
    #[must_use]
    pub const fn ingress_available(&self) -> bool {
        matches!(
            self.state,
            ServiceState::Listening | ServiceState::Authenticating | ServiceState::Authenticated
        ) && self.packet_inflight.is_none()
    }

    /// Whether the one-slot root-control record may be published.
    #[must_use]
    pub const fn control_available(&self) -> bool {
        !matches!(self.state, ServiceState::Terminal | ServiceState::Faulted)
            && self.control_inflight.is_none()
    }

    /// Return the exact staged control record whose child-consumption
    /// watermark has not yet been accepted by root.
    #[must_use]
    pub fn control_publication_owed(&self) -> Option<ConsoleNetworkControlPublication> {
        let sequence = self.control_inflight?;
        let connection_id = self.last_control_connection_id?;
        (sequence != 0
            && sequence == self.last_control_issued
            && connection_id != 0
            && !matches!(self.state, ServiceState::Terminal | ServiceState::Faulted))
        .then_some(ConsoleNetworkControlPublication {
            generation: self.generation,
            connection_id,
            sequence,
        })
    }

    /// Whether any child-to-root publication is newer than root's accepted
    /// frontier.
    ///
    /// Semantic events and packet egress share the global publication credit;
    /// both input-completion watermarks are independent sequence-last levels.
    /// All four feed the same coalescing notification, so the final
    /// condition-before-block cut must inspect every level without accepting
    /// any of them.
    pub fn child_publication_pending(
        &self,
        event_page: &[u8],
        egress_page: &[u8],
    ) -> Result<bool, BoundaryError> {
        let event_pending = ExchangePage::publication_pending(
            event_page,
            self.generation,
            self.last_event_sequence,
        )?;
        let egress_pending = PacketPage::publication_pending(
            egress_page,
            PacketDirection::Egress,
            self.generation,
            self.last_egress_sequence,
        )?;
        let watermarks = ExchangePage::completion_watermarks(event_page, self.generation)?;
        let ingress_completion_pending = Self::input_completion_pending(
            watermarks.ingress_sequence,
            self.last_packet_consumed_sequence,
            self.packet_inflight,
        )?;
        let control_completion_pending = Self::input_completion_pending(
            watermarks.control_sequence,
            self.last_control_consumed_sequence,
            self.control_inflight,
        )?;
        Ok(event_pending
            || egress_pending
            || ingress_completion_pending
            || control_completion_pending)
    }

    fn input_completion_pending(
        observed: u64,
        accepted: u64,
        inflight: Option<u64>,
    ) -> Result<bool, BoundaryError> {
        if observed < accepted {
            return Err(BoundaryError::StaleIdentity);
        }
        if observed == accepted {
            return Ok(false);
        }
        if inflight == Some(observed) {
            Ok(true)
        } else {
            Err(BoundaryError::StaleIdentity)
        }
    }

    /// Whether the event page contains a semantic publication newer than the
    /// last one already copied by root.
    pub fn event_publication_pending(&self, input_page: &[u8]) -> Result<bool, BoundaryError> {
        ExchangePage::publication_pending(input_page, self.generation, self.last_event_sequence)
            .map_err(Into::into)
    }

    /// Retire exact packet/control input ownership from the negotiated
    /// child-published watermark pair.
    pub fn accept_completion_watermarks(
        &mut self,
        input_page: &[u8],
    ) -> Result<ConsoleNetworkInputCompletions, BoundaryError> {
        let watermarks = ExchangePage::completion_watermarks(input_page, self.generation)?;
        if watermarks.ingress_sequence < self.last_packet_consumed_sequence
            || watermarks.control_sequence < self.last_control_consumed_sequence
        {
            return Err(BoundaryError::StaleIdentity);
        }

        let packet_sequence = if watermarks.ingress_sequence == self.last_packet_consumed_sequence {
            None
        } else if self.packet_inflight == Some(watermarks.ingress_sequence) {
            Some(watermarks.ingress_sequence)
        } else {
            return Err(BoundaryError::StaleIdentity);
        };
        let control_sequence = if watermarks.control_sequence == self.last_control_consumed_sequence
        {
            None
        } else if self.control_inflight == Some(watermarks.control_sequence) {
            Some(watermarks.control_sequence)
        } else {
            return Err(BoundaryError::StaleIdentity);
        };

        if let Some(sequence) = packet_sequence {
            self.packet_inflight = None;
            self.last_packet_consumed_sequence = sequence;
        }
        if let Some(sequence) = control_sequence {
            self.control_inflight = None;
            self.last_control_consumed_sequence = sequence;
        }
        Ok(ConsoleNetworkInputCompletions {
            packet_sequence,
            control_sequence,
        })
    }

    /// Publish one copied NIC packet, preserving one-slot backpressure.
    pub fn stage_ingress(
        &mut self,
        packet: &[u8],
        output_page: &mut [u8],
    ) -> Result<u64, BoundaryError> {
        if !matches!(
            self.state,
            ServiceState::Listening | ServiceState::Authenticating | ServiceState::Authenticated
        ) {
            return Err(BoundaryError::InvalidState);
        }
        if self.packet_inflight.is_some() {
            return Err(BoundaryError::Backpressure);
        }
        let sequence = self.next_packet_sequence;
        PacketPage::publish_into(
            output_page,
            PacketDirection::Ingress,
            self.generation,
            sequence,
            packet,
        )?;
        self.packet_inflight = Some(sequence);
        self.next_packet_sequence = next_sequence(sequence);
        Ok(sequence)
    }

    /// Publish one root-authorized output line.
    pub fn stage_authorized_line(
        &mut self,
        line: &str,
        now_ms: u64,
        output_page: &mut [u8],
    ) -> Result<u64, BoundaryError> {
        let connection_id = self
            .authenticated_connection()
            .ok_or(BoundaryError::InvalidState)?;
        self.stage_control(
            ExchangeKind::SendLine,
            connection_id,
            now_ms,
            line.as_bytes(),
            output_page,
        )
    }

    /// Publish one validated root-authorized response batch.
    pub fn stage_authorized_batch(
        &mut self,
        payload: &[u8],
        now_ms: u64,
        output_page: &mut [u8],
    ) -> Result<u64, BoundaryError> {
        let connection_id = self
            .authenticated_connection()
            .ok_or(BoundaryError::InvalidState)?;
        console_network_abi::SendBatchCursor::validate(payload)?;
        self.stage_control(
            ExchangeKind::SendBatch,
            connection_id,
            now_ms,
            payload,
            output_page,
        )
    }

    /// Publish a close-after-flush request for the active connection.
    pub fn stage_disconnect(
        &mut self,
        now_ms: u64,
        output_page: &mut [u8],
    ) -> Result<u64, BoundaryError> {
        let connection_id = self.connection_id.ok_or(BoundaryError::InvalidState)?;
        self.stage_control(
            ExchangeKind::Disconnect,
            connection_id,
            now_ms,
            &[],
            output_page,
        )
    }

    fn stage_control(
        &mut self,
        kind: ExchangeKind,
        connection_id: u64,
        now_ms: u64,
        payload: &[u8],
        output_page: &mut [u8],
    ) -> Result<u64, BoundaryError> {
        if self.control_inflight.is_some() {
            return Err(BoundaryError::Backpressure);
        }
        let sequence = self.next_control_sequence;
        ExchangePage::publish_related_into(
            output_page,
            kind,
            self.generation,
            sequence,
            connection_id,
            now_ms,
            0,
            payload,
        )?;
        self.control_inflight = Some(sequence);
        self.last_control_issued = sequence;
        self.last_control_connection_id = Some(connection_id);
        self.next_control_sequence = next_sequence(sequence);
        Ok(sequence)
    }

    /// Validate one child event and apply its exact lifecycle transition.
    pub fn accept_event(
        &mut self,
        input_page: &[u8],
    ) -> Result<ConsoleNetworkEvent, BoundaryError> {
        let record = ExchangePage::decode_bounded(
            input_page,
            self.generation,
            self.last_event_sequence,
            false,
        )?;
        let kind = record.kind();
        let payload = record.payload();
        let mut owned_payload = HeaplessVec::new();
        owned_payload
            .extend_from_slice(payload)
            .map_err(|_| BoundaryError::InvalidRecord)?;
        match kind {
            ExchangeKind::Ready => {
                if self.state != ServiceState::Constructing || payload != READY_IDENTITY.as_bytes()
                {
                    return Err(BoundaryError::InvalidState);
                }
                self.state = ServiceState::Listening;
            }
            ExchangeKind::Connected => {
                if self.state != ServiceState::Listening || record.connection_id() == 0 {
                    return Err(BoundaryError::InvalidState);
                }
                self.connection_id = Some(record.connection_id());
                self.last_control_connection_id = None;
                self.last_output_drained_sequence = 0;
                self.last_output_drained_connection_id = None;
                self.state = ServiceState::Authenticating;
            }
            ExchangeKind::Authenticated => {
                if self.state != ServiceState::Authenticating
                    || self.connection_id != Some(record.connection_id())
                {
                    return Err(BoundaryError::InvalidState);
                }
                self.state = ServiceState::Authenticated;
            }
            ExchangeKind::Command => {
                if self.state != ServiceState::Authenticated
                    || self.connection_id != Some(record.connection_id())
                {
                    return Err(BoundaryError::InvalidState);
                }
            }
            ExchangeKind::CommandBatch => {
                if self.state != ServiceState::Authenticated
                    || self.connection_id != Some(record.connection_id())
                    || CommandBatchCursor::validate(payload).is_err()
                {
                    return Err(BoundaryError::InvalidState);
                }
            }
            ExchangeKind::Disconnected => {
                if self.connection_id != Some(record.connection_id()) {
                    return Err(BoundaryError::StaleIdentity);
                }
                self.connection_id = None;
                self.last_control_connection_id = None;
                self.last_output_drained_sequence = 0;
                self.last_output_drained_connection_id = None;
                if self.state != ServiceState::Closing {
                    self.state = ServiceState::Listening;
                }
            }
            ExchangeKind::Rejected | ExchangeKind::Backpressure => {
                if self.connection_id != Some(record.connection_id()) {
                    return Err(BoundaryError::StaleIdentity);
                }
            }
            ExchangeKind::PacketConsumed => {
                return Err(BoundaryError::InvalidRecord);
            }
            ExchangeKind::ControlCompleted => {
                return Err(BoundaryError::InvalidRecord);
            }
            ExchangeKind::OutputDrained => {
                if self.connection_id != Some(record.connection_id())
                    || self.last_control_connection_id != Some(record.connection_id())
                    || record.related_sequence() > self.last_control_issued
                    || record.related_sequence() <= self.last_output_drained_sequence
                {
                    return Err(BoundaryError::StaleIdentity);
                }
                self.last_output_drained_sequence = record.related_sequence();
                self.last_output_drained_connection_id = Some(record.connection_id());
            }
            ExchangeKind::ShutdownComplete => {
                if !matches!(self.state, ServiceState::Closing | ServiceState::Faulted) {
                    return Err(BoundaryError::InvalidState);
                }
                self.state = ServiceState::Terminal;
                self.connection_id = None;
            }
            ExchangeKind::SendLine | ExchangeKind::SendBatch | ExchangeKind::Disconnect => {
                return Err(BoundaryError::InvalidRecord)
            }
        }
        self.last_event_sequence = record.sequence();
        Ok(ConsoleNetworkEvent {
            kind,
            connection_id: record.connection_id(),
            now_ms: record.now_ms(),
            related_sequence: record.related_sequence(),
            payload: owned_payload,
        })
    }

    /// Validate and copy one child-generated Ethernet packet.
    pub fn accept_egress(
        &mut self,
        input_page: &[u8],
    ) -> Result<HeaplessVec<u8, ETHERNET_FRAME_BYTES>, BoundaryError> {
        let record =
            PacketPage::decode_bounded(input_page, self.generation, self.last_egress_sequence)?;
        if record.direction() != PacketDirection::Egress {
            return Err(BoundaryError::InvalidRecord);
        }
        let mut copied = HeaplessVec::new();
        copied
            .extend_from_slice(record.packet())
            .map_err(|_| BoundaryError::InvalidRecord)?;
        self.last_egress_sequence = record.sequence();
        Ok(copied)
    }

    /// Stop admission before sending the generated shutdown badge.
    pub fn begin_shutdown(&mut self) -> Result<(), BoundaryError> {
        if matches!(self.state, ServiceState::Terminal | ServiceState::Faulted) {
            return Err(BoundaryError::InvalidState);
        }
        self.state = ServiceState::Closing;
        Ok(())
    }

    /// Record timeout or standard child fault and stop all new work.
    pub fn record_fault(&mut self) {
        if self.state != ServiceState::Terminal {
            self.state = ServiceState::Faulted;
        }
    }

    /// Fence the old generation only after complete kernel containment.
    pub fn reconstruct(
        &mut self,
        proof: ConsoleNetworkContainmentProof,
    ) -> Result<u64, BoundaryError> {
        if !matches!(self.state, ServiceState::Faulted | ServiceState::Terminal)
            || !proof.complete()
        {
            return Err(BoundaryError::IncompleteContainment);
        }
        self.generation = next_sequence(self.generation);
        self.state = ServiceState::Constructing;
        self.next_packet_sequence = 1;
        self.next_control_sequence = 1;
        self.last_event_sequence = 0;
        self.last_egress_sequence = 0;
        self.last_packet_consumed_sequence = 0;
        self.last_control_consumed_sequence = 0;
        self.packet_inflight = None;
        self.control_inflight = None;
        self.connection_id = None;
        self.last_control_issued = 0;
        self.last_control_connection_id = None;
        self.last_output_drained_sequence = 0;
        self.last_output_drained_connection_id = None;
        Ok(self.generation)
    }

    /// Whether network work can consume another root or emergency SC.
    #[must_use]
    pub const fn borrows_root_scheduling_context(&self) -> bool {
        false
    }

    /// Whether another target TCP listener is permitted.
    #[must_use]
    pub const fn permits_second_listener(&self) -> bool {
        false
    }

    /// Whether the newest control for this connection has left smoltcp's send queue.
    #[must_use]
    pub fn console_output_drained(&self, connection_id: u64) -> bool {
        connection_id != 0
            && self.connection_id == Some(connection_id)
            && self.control_inflight.is_none()
            && self.last_control_connection_id == Some(connection_id)
            && self.last_output_drained_connection_id == Some(connection_id)
            && self.last_control_issued != 0
            && self.last_output_drained_sequence == self.last_control_issued
    }
}

fn next_sequence(sequence: u64) -> u64 {
    sequence.saturating_add(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::{
        console_network_signal_only_admitted, console_network_yield_to_admitted,
        expected_object_inventory, expected_runtime_image_pages,
        select_console_network_cross_core_signal_only,
        validate_console_network_signal_only_topology, BoundaryError,
        ConsoleNetworkSignalOnlyAdmission, ConsoleNetworkSignalOnlyTopology,
        ConsoleNetworkYieldToAdmission, ServiceState,
    };
    use console_network_abi::{WAKE_CONTROL, WAKE_PACKET_RX, WAKE_PUBLICATION_ACK};

    fn exact_yield_to_admission() -> ConsoleNetworkYieldToAdmission {
        ConsoleNetworkYieldToAdmission {
            profile_enabled: true,
            runtime_direct_virtio: false,
            runtime_direct_genet: true,
            service_state: ServiceState::Authenticated,
            durable_publication: true,
            signal_badge: WAKE_CONTROL,
            activated: true,
            containment_started: false,
            contained: false,
            scheduling_context_present: true,
        }
    }

    #[test]
    fn exact_backend_selects_qemu_pi_and_mediated_object_inventories() {
        assert_eq!(expected_runtime_image_pages(true, false), Some(62));
        assert_eq!(expected_object_inventory(true, false), Ok((134, 162)));
        assert_eq!(expected_runtime_image_pages(false, true), Some(66));
        assert_eq!(expected_object_inventory(false, true), Ok((104, 161)));
        assert_eq!(expected_runtime_image_pages(false, false), Some(60));
        assert_eq!(expected_object_inventory(false, false), Ok((98, 123)));
        assert_eq!(expected_runtime_image_pages(true, true), None);
        assert!(expected_object_inventory(true, true).is_err());
    }

    #[test]
    fn pi_direct_genet_requires_exact_cross_core_signal_only_topology() {
        let exact = ConsoleNetworkSignalOnlyTopology {
            required: true,
            root_core: 0,
            root_sched_control_core: 0,
            root_priority: 200,
            root_mcp: 200,
            child_core: 2,
            child_sched_control_core: 2,
            child_priority: 200,
            child_scheduling_context_slot: 6,
        };
        assert_eq!(validate_console_network_signal_only_topology(exact), Ok(()));
        assert_eq!(
            validate_console_network_signal_only_topology(ConsoleNetworkSignalOnlyTopology {
                required: false,
                child_priority: 180,
                ..exact
            }),
            Ok(()),
            "QEMU keeps its existing signal-only scheduling behavior"
        );
        for invalid in [
            ConsoleNetworkSignalOnlyTopology {
                child_core: 0,
                child_sched_control_core: 0,
                ..exact
            },
            ConsoleNetworkSignalOnlyTopology {
                child_core: 1,
                child_sched_control_core: 1,
                ..exact
            },
            ConsoleNetworkSignalOnlyTopology {
                child_priority: 199,
                ..exact
            },
            ConsoleNetworkSignalOnlyTopology {
                root_mcp: 199,
                ..exact
            },
            ConsoleNetworkSignalOnlyTopology {
                child_scheduling_context_slot: 0,
                ..exact
            },
        ] {
            assert_eq!(
                validate_console_network_signal_only_topology(invalid),
                Err(BoundaryError::TemporalDrift)
            );
        }
    }

    #[test]
    fn direct_genet_alone_selects_cross_core_signal_only() {
        assert_eq!(
            select_console_network_cross_core_signal_only(true, false),
            Ok(false),
            "QEMU direct VirtIO retains its existing signal-only scheduling"
        );
        assert_eq!(
            select_console_network_cross_core_signal_only(false, true),
            Ok(true),
            "Pi direct GENET selects exact cross-core signal-only handoff"
        );
        assert_eq!(
            select_console_network_cross_core_signal_only(false, false),
            Ok(false),
            "mediated Pi WiFi retains the proven signal-only handoff"
        );
        assert_eq!(
            select_console_network_cross_core_signal_only(true, true),
            Err(BoundaryError::GeneratedDrift),
            "mutually exclusive direct backends fail closed"
        );
    }

    #[test]
    fn signal_only_runtime_admission_preserves_pi_lifecycle_and_qemu_isolation() {
        let exact_pi = ConsoleNetworkSignalOnlyAdmission {
            contract_direct_genet: true,
            runtime_direct_genet: true,
            cross_core_signal_only: true,
            yield_to_child_after_signal: false,
            activated: true,
            containment_started: false,
            contained: false,
            scheduling_context_present: true,
        };
        assert!(console_network_signal_only_admitted(exact_pi));
        assert!(!console_network_signal_only_admitted(
            ConsoleNetworkSignalOnlyAdmission {
                runtime_direct_genet: false,
                ..exact_pi
            }
        ));
        assert!(console_network_signal_only_admitted(
            ConsoleNetworkSignalOnlyAdmission {
                contract_direct_genet: false,
                runtime_direct_genet: false,
                cross_core_signal_only: false,
                activated: false,
                containment_started: true,
                contained: true,
                scheduling_context_present: false,
                ..exact_pi
            }
        ));
        for invalid in [
            ConsoleNetworkSignalOnlyAdmission {
                contract_direct_genet: false,
                runtime_direct_genet: true,
                cross_core_signal_only: false,
                ..exact_pi
            },
            ConsoleNetworkSignalOnlyAdmission {
                cross_core_signal_only: false,
                ..exact_pi
            },
            ConsoleNetworkSignalOnlyAdmission {
                yield_to_child_after_signal: true,
                ..exact_pi
            },
            ConsoleNetworkSignalOnlyAdmission {
                activated: false,
                ..exact_pi
            },
            ConsoleNetworkSignalOnlyAdmission {
                containment_started: true,
                ..exact_pi
            },
            ConsoleNetworkSignalOnlyAdmission {
                contained: true,
                ..exact_pi
            },
            ConsoleNetworkSignalOnlyAdmission {
                scheduling_context_present: false,
                ..exact_pi
            },
        ] {
            assert!(!console_network_signal_only_admitted(invalid));
        }
    }

    #[test]
    fn yield_to_requires_post_auth_durable_exact_one_hot_work_and_live_child() {
        let exact = exact_yield_to_admission();
        assert!(console_network_yield_to_admitted(exact));
        assert!(console_network_yield_to_admitted(
            ConsoleNetworkYieldToAdmission {
                signal_badge: WAKE_PUBLICATION_ACK,
                ..exact
            }
        ));
        for invalid in [
            ConsoleNetworkYieldToAdmission {
                profile_enabled: false,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                runtime_direct_virtio: true,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                runtime_direct_genet: false,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                service_state: ServiceState::Authenticating,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                service_state: ServiceState::Closing,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                durable_publication: false,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                signal_badge: 0,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                signal_badge: WAKE_CONTROL | WAKE_PUBLICATION_ACK,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                signal_badge: WAKE_PACKET_RX,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                activated: false,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                containment_started: true,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                contained: true,
                ..exact
            },
            ConsoleNetworkYieldToAdmission {
                scheduling_context_present: false,
                ..exact
            },
        ] {
            assert!(!console_network_yield_to_admitted(invalid));
        }
    }
}
