// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Construct and contain the generated isolated console-network child.
// Author: Lukas Bower

//! HAL-owned construction for `console-network-service/v6`.
//!
//! Every child object and translation table is retyped below one retained
//! compiler-selected revoke anchor. The root keeps only copied packet/control
//! pages, one receive notification, five protocol send caps, the physical
//! root-control fan-in send cap, and the TCB/SC caps needed for supervision.
//! Construction leaves the TCB suspended until the complete target fault
//! registry has been sealed.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicU64, Ordering};

#[cfg(any(feature = "net-backend-virtio", test))]
use console_network_abi::WAKE_DIRECT_VIRTIO_IRQ;
use console_network_abi::{
    DirectGenetControlPage, DirectGenetLayout, DirectGenetRuntimeDiagnostic, DirectGenetSlotPage,
    DirectVirtioLayout, ExchangePage, PacketDirection, PacketPage, CHILD_WAKE_MASK,
    DIRECT_GENET_LAYOUT_BYTES, DIRECT_GENET_LAYOUT_OFFSET, DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES,
    DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET, DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET,
    DIRECT_GENET_RX_SLOT_COUNT, DIRECT_GENET_SHARED_PAGE_COUNT, DIRECT_GENET_TX_SLOT_COUNT,
    DIRECT_VIRTIO_BUFFER_COUNT, DIRECT_VIRTIO_LAYOUT_BYTES, DIRECT_VIRTIO_LAYOUT_OFFSET,
    DIRECT_VIRTIO_QUEUE_COUNT, ROOT_CONTROL_WAKE_NOTIFICATION_BADGE,
    ROOT_CONTROL_WAKE_NOTIFICATION_SLOT, RUNTIME_INIT_DESCRIPTOR_BYTES, SHARED_PAGE_BYTES,
    WAKE_CONTROL, WAKE_EVENT_READY, WAKE_PACKET_RX, WAKE_PACKET_TX_READY, WAKE_PUBLICATION_ACK,
    WAKE_REVOKE, WAKE_SHUTDOWN,
};
#[cfg(feature = "net-backend-genet-direct")]
use console_network_abi::{
    DIRECT_GENET_LAYOUT_FLAGS, DIRECT_GENET_LAYOUT_MAGIC, DIRECT_GENET_LAYOUT_VERSION,
    DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT,
};
#[cfg(feature = "net-backend-virtio")]
use console_network_abi::{
    DIRECT_VIRTIO_IRQ_HANDLER_SLOT, DIRECT_VIRTIO_LAYOUT_MAGIC, DIRECT_VIRTIO_LAYOUT_VERSION,
    DIRECT_VIRTIO_PAGE_BYTES, DIRECT_VIRTIO_QUEUE_SIZE,
};
use heapless::Vec;
use sel4_sys::{seL4_CPtr, seL4_Word};

use super::{
    fill_runtime_elf_page, plan_runtime_elf_load, runtime_cacheable_xn_attributes,
    runtime_elf_page_mapping, runtime_uncached_xn_attributes, HalError, KernelHal,
};
use crate::console_network_service::{
    console_network_signal_only_admitted, console_network_yield_to_admitted,
    expected_runtime_image_pages, BoundaryError, ConsoleNetworkBoundary,
    ConsoleNetworkContainmentCursor, ConsoleNetworkContainmentProof, ConsoleNetworkContainmentTurn,
    ConsoleNetworkContainmentUnit, ConsoleNetworkContract, ConsoleNetworkEvent,
    ConsoleNetworkObjectPlan, ConsoleNetworkSignalOnlyAdmission, ConsoleNetworkYieldToAdmission,
    ServiceState, CONSOLE_NETWORK_IMAGE_IDENTITY_BOUND, CONSOLE_NETWORK_RUNTIME_ENTRY_VADDR,
    CONSOLE_NETWORK_RUNTIME_IMAGE, CONSOLE_NETWORK_RUNTIME_LOAD_BASE_VADDR,
    CONSOLE_NETWORK_RUNTIME_LOAD_LIMIT_VADDR, CONSOLE_NETWORK_RUNTIME_LOAD_PAGES, SERVICE_TASK_ID,
};
use crate::critical_tcb::GenerationIdentity;
use crate::net::DirectGenetYieldAccounting;
#[cfg(all(
    feature = "kernel",
    feature = "release-pi4",
    feature = "net-backend-genet-direct",
    target_arch = "aarch64",
    target_os = "none",
    sel4_config_kernel_mcs
))]
use crate::net::DIRECT_GENET_YIELD_ACCOUNTING_INVALID_PREDRAIN;
use crate::sel4::{self, RamFrame, RevokeAnchorVSpaceTracker};

const TRANSLATION_SLOT_COUNT: usize = 8;
#[cfg(feature = "net-backend-virtio")]
const DIRECT_DMA_FRAME_COUNT: usize = DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT * 2;
#[cfg(not(feature = "net-backend-virtio"))]
const DIRECT_DMA_FRAME_COUNT: usize = 0;
#[cfg(feature = "net-backend-virtio")]
const DIRECT_DEVICE_SLOT_COUNT: usize = 1;
#[cfg(not(feature = "net-backend-virtio"))]
const DIRECT_DEVICE_SLOT_COUNT: usize = 0;

#[cfg(feature = "net-backend-virtio")]
const fn direct_virtio_dma_attributes() -> sel4_sys::seL4_ARM_VMAttributes {
    // QEMU accesses guest RAM through a normal cacheable host mapping. The
    // AArch64 guest must use the matching normal-memory attribute so KVM/HVF
    // and QEMU observe one coherent cache domain; the VirtIO barriers retain
    // descriptor/index ordering. Only the MMIO page is device memory.
    runtime_cacheable_xn_attributes()
}

#[cfg(feature = "net-backend-virtio")]
const fn direct_virtio_mmio_attributes() -> sel4_sys::seL4_ARM_VMAttributes {
    runtime_uncached_xn_attributes()
}
#[cfg(feature = "net-backend-virtio")]
const DIRECT_IRQ_SLOT_COUNT: usize = 2;
#[cfg(not(feature = "net-backend-virtio"))]
const DIRECT_IRQ_SLOT_COUNT: usize = 0;
const IMAGE_FRAME_COUNT: usize = match expected_runtime_image_pages(
    cfg!(feature = "net-backend-virtio"),
    cfg!(feature = "net-backend-genet-direct"),
) {
    Some(pages) => pages as usize,
    None => 0,
};
const BASE_FRAME_COUNT: usize = IMAGE_FRAME_COUNT + 38;
const FRAME_COUNT: usize = BASE_FRAME_COUNT + DIRECT_DMA_FRAME_COUNT;
const IMAGE_FRAME_START: usize = 0;
const STACK_FRAME_START: usize = IMAGE_FRAME_COUNT;
const IPC_FRAME_INDEX: usize = STACK_FRAME_START + 32;
const INIT_FRAME_INDEX: usize = IPC_FRAME_INDEX + 1;
const SHARED_FRAME_START: usize = INIT_FRAME_INDEX + 1;
const SHARED_FRAME_COUNT: usize = 4;
const DIRECT_FRAME_START: usize = BASE_FRAME_COUNT;
const DIRECT_QUEUE_FRAME_START: usize = DIRECT_FRAME_START;
const DIRECT_RX_FRAME_START: usize = DIRECT_QUEUE_FRAME_START + DIRECT_VIRTIO_QUEUE_COUNT;
const DIRECT_TX_FRAME_START: usize = DIRECT_RX_FRAME_START + DIRECT_VIRTIO_BUFFER_COUNT;

#[cfg(feature = "net-backend-genet-direct")]
const DIRECT_GENET_FRAME_COPY_SLOT_COUNT: usize = DIRECT_GENET_SHARED_PAGE_COUNT;
#[cfg(not(feature = "net-backend-genet-direct"))]
const DIRECT_GENET_FRAME_COPY_SLOT_COUNT: usize = 0;
const DIRECT_GENET_CONTROL_VADDR: usize = 0x7208_0000;
const DIRECT_GENET_RX_VADDR: usize = DIRECT_GENET_CONTROL_VADDR + SHARED_PAGE_BYTES;
const DIRECT_GENET_TX_VADDR: usize =
    DIRECT_GENET_RX_VADDR + DIRECT_GENET_RX_SLOT_COUNT * SHARED_PAGE_BYTES;

fn sample_direct_genet_runtime_diagnostic(
    control_root_ptr: usize,
    generation: u64,
) -> Option<DirectGenetRuntimeDiagnostic> {
    if control_root_ptr == 0
        || !control_root_ptr.is_multiple_of(SHARED_PAGE_BYTES)
        || generation == 0
    {
        return None;
    }
    let diagnostic_ptr = control_root_ptr.checked_add(DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET)?;
    const DIAGNOSTIC_WORD_COUNT: usize = DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES / 8;
    const COMMIT_WORD_INDEX: usize = DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET / 8;
    // SAFETY: Construction retains a page-aligned root mapping for the exact
    // CPU-only control frame. This page-local diagnostic region is 64-bit
    // aligned, has exactly `DIAGNOSTIC_WORD_COUNT` initialized words, and both
    // participants access every overlapping word through `AtomicU64`. The
    // child is the sole writer and root retains only this observational view.
    let diagnostic_words =
        unsafe { &*(diagnostic_ptr as *const [AtomicU64; DIAGNOSTIC_WORD_COUNT]) };
    let first_commit = diagnostic_words[COMMIT_WORD_INDEX].load(Ordering::Acquire);
    if first_commit == 0 {
        return None;
    }
    let mut encoded = [0u8; DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES];
    let mut offset = 0usize;
    while offset < DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET {
        let word = diagnostic_words[offset / 8].load(Ordering::Relaxed);
        encoded[offset..offset + 8].copy_from_slice(&word.to_le_bytes());
        offset += 8;
    }
    // Complete the relaxed body reads before rechecking the commit. The
    // writer orders commit invalidation before its body stores; this acquire
    // fence prevents accepting a mixed body under two old commit samples.
    // An acquire load alone would order only the reads that follow it.
    fence(Ordering::Acquire);
    let second_commit = diagnostic_words[COMMIT_WORD_INDEX].load(Ordering::Acquire);
    if second_commit != first_commit {
        return None;
    }
    encoded[DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET
        ..DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET + 8]
        .copy_from_slice(&second_commit.to_le_bytes());
    DirectGenetRuntimeDiagnostic::decode(&encoded, generation).ok()
}

const DIRECT_VIRTIO_MMIO_PADDR: usize = 0x0a00_0000;
const DIRECT_VIRTIO_MMIO_VADDR: usize = 0x7205_0000;
const DIRECT_VIRTIO_QUEUE_VADDR: usize = 0x7205_1000;
const DIRECT_VIRTIO_RX_VADDR: usize = 0x7205_3000;
const DIRECT_VIRTIO_TX_VADDR: usize = 0x7206_3000;
#[cfg(feature = "net-backend-virtio")]
const DIRECT_VIRTIO_IRQ: seL4_Word = 48;

const TCB_SLOT_INDEX: usize = 0;
const CNODE_SLOT_INDEX: usize = 1;
const VSPACE_SLOT_INDEX: usize = 2;
const SC_SLOT_INDEX: usize = 3;
const ROOT_TO_CHILD_NOTIFICATION_INDEX: usize = 4;
const CHILD_TO_ROOT_NOTIFICATION_INDEX: usize = 5;
const FRAME_SLOT_START: usize = 6;
const TRANSLATION_SLOT_START: usize = FRAME_SLOT_START + FRAME_COUNT;
const SHARED_COPY_SLOT_START: usize = TRANSLATION_SLOT_START + TRANSLATION_SLOT_COUNT;
const ROOT_WAKE_SLOT_START: usize = SHARED_COPY_SLOT_START + SHARED_FRAME_COUNT;
const STANDARD_FAULT_SLOT_INDEX: usize = ROOT_WAKE_SLOT_START + 5;
const TIMEOUT_FAULT_SLOT_INDEX: usize = STANDARD_FAULT_SLOT_INDEX + 1;
const DIRECT_GENET_FRAME_COPY_SLOT_START: usize = TIMEOUT_FAULT_SLOT_INDEX + 1;
const DIRECT_MMIO_SLOT_INDEX: usize =
    DIRECT_GENET_FRAME_COPY_SLOT_START + DIRECT_GENET_FRAME_COPY_SLOT_COUNT;
const DIRECT_IRQ_NOTIFICATION_SLOT_INDEX: usize = DIRECT_MMIO_SLOT_INDEX + DIRECT_DEVICE_SLOT_COUNT;
const DIRECT_IRQ_HANDLER_SLOT_INDEX: usize = DIRECT_IRQ_NOTIFICATION_SLOT_INDEX + 1;
const ROOT_SLOT_COUNT: usize = TIMEOUT_FAULT_SLOT_INDEX
    + 1
    + DIRECT_GENET_FRAME_COPY_SLOT_COUNT
    + DIRECT_DEVICE_SLOT_COUNT
    + DIRECT_IRQ_SLOT_COUNT;

const ROOT_PACKET_RX_WAKE_INDEX: usize = 0;
const ROOT_CONTROL_WAKE_INDEX: usize = 1;
const ROOT_SHUTDOWN_WAKE_INDEX: usize = 2;
const ROOT_REVOKE_WAKE_INDEX: usize = 3;
const ROOT_PUBLICATION_ACK_WAKE_INDEX: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DurableChildPublication {
    None,
    AuthenticatedControl,
    AuthenticatedPublicationCredit,
}

impl DurableChildPublication {
    const fn committed_for_badge(self, signal_badge: u64) -> Result<bool, BoundaryError> {
        match self {
            Self::None => Ok(false),
            Self::AuthenticatedControl if signal_badge == WAKE_CONTROL => Ok(true),
            Self::AuthenticatedPublicationCredit if signal_badge == WAKE_PUBLICATION_ACK => {
                Ok(true)
            }
            Self::AuthenticatedControl | Self::AuthenticatedPublicationCredit => {
                Err(BoundaryError::HandoffFailed)
            }
        }
    }
}

const fn root_wake_badge(wake_index: usize) -> Option<u64> {
    match wake_index {
        ROOT_PACKET_RX_WAKE_INDEX => Some(WAKE_PACKET_RX),
        ROOT_CONTROL_WAKE_INDEX => Some(WAKE_CONTROL),
        ROOT_SHUTDOWN_WAKE_INDEX => Some(WAKE_SHUTDOWN),
        ROOT_REVOKE_WAKE_INDEX => Some(WAKE_REVOKE),
        ROOT_PUBLICATION_ACK_WAKE_INDEX => Some(WAKE_PUBLICATION_ACK),
        _ => None,
    }
}

/// Return whether this committed signal will enter the exact direct-GENET
/// same-core child call and therefore requires a fresh SC accounting drain.
/// Mediated WiFi shares the Pi binary but must remain signal-only.
const fn direct_genet_predrain_required(runtime_direct_genet: bool, yield_admitted: bool) -> bool {
    runtime_direct_genet && yield_admitted
}

/// Return whether the exact same-core child call may follow its SC pre-drain.
///
/// A failed drain makes child-consumed accounting ambiguous, so the durable
/// notification is still delivered but the optional YieldTo optimization must
/// fail closed.
const fn direct_genet_yield_after_predrain_admitted(
    yield_admitted: bool,
    predrain_succeeded: bool,
) -> bool {
    yield_admitted && predrain_succeeded
}

const CHILD_CNODE_RADIX_BITS: u8 = 4;

const _: () = assert!(TIMEOUT_FAULT_SLOT_INDEX < ROOT_SLOT_COUNT);
const _: () = assert!(IMAGE_FRAME_COUNT != 0);
#[cfg(feature = "net-backend-virtio")]
const _: () = assert!(DIRECT_MMIO_SLOT_INDEX + 1 == DIRECT_IRQ_NOTIFICATION_SLOT_INDEX);
#[cfg(feature = "net-backend-virtio")]
const _: () = assert!(DIRECT_IRQ_HANDLER_SLOT_INDEX + 1 == ROOT_SLOT_COUNT);
#[cfg(feature = "net-backend-genet-direct")]
const _: () = assert!(DIRECT_GENET_FRAME_COPY_SLOT_START + 32 == ROOT_SLOT_COUNT);
const _: () = assert!(STACK_FRAME_START + 32 == IPC_FRAME_INDEX);
const _: () = assert!(SHARED_FRAME_START + SHARED_FRAME_COUNT == BASE_FRAME_COUNT);
#[cfg(feature = "net-backend-virtio")]
const _: () = assert!(DIRECT_TX_FRAME_START + DIRECT_VIRTIO_BUFFER_COUNT == FRAME_COUNT);
const _: () = assert!(SHARED_FRAME_COUNT == ConsoleNetworkContainmentCursor::SHARED_FRAME_COUNT);
const _: () = assert!(ConsoleNetworkContainmentCursor::FAULT_CAP_COUNT == 2);
const _: () = assert!(
    DIRECT_GENET_TX_VADDR + DIRECT_GENET_TX_SLOT_COUNT * SHARED_PAGE_BYTES
        == DIRECT_GENET_CONTROL_VADDR + DIRECT_GENET_SHARED_PAGE_COUNT * SHARED_PAGE_BYTES
);

/// Whether this generated policy installs a TCB timeout endpoint.
///
/// Natural postponement retains the compiler-reserved timeout capability and
/// SC badge, but deliberately leaves the TCB handler slot empty so seL4 delays
/// the thread until its next replenishment. Every fault-delivering policy keeps
/// the existing endpoint installation path.
const fn requires_timeout_endpoint(policy: crate::generated::TimeoutPolicy) -> bool {
    !matches!(policy, crate::generated::TimeoutPolicy::NaturalPostpone)
}

/// One nonblocking child-output turn copied into root-owned values.
pub struct ConsoleNetworkTurn {
    /// Exact root inputs newly consumed by the child.
    pub input_completions: crate::console_network_service::ConsoleNetworkInputCompletions,
    /// Validated service event, when the event-ready bit was observed.
    pub event: Option<ConsoleNetworkEvent>,
    /// Validated Ethernet frame, when the packet-ready bit was observed.
    pub egress: Option<Vec<u8, { console_network_abi::ETHERNET_FRAME_BYTES }>>,
}

/// Exact validation stage that rejected one child-output observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleNetworkPollStage {
    /// Runtime lifecycle cannot admit observation.
    State,
    /// Notification badge contains undeclared authority.
    Badge,
    /// Child-published input-completion pair is invalid.
    CompletionWatermarks,
    /// Event-page committed sequence is not a valid readiness observation.
    EventReadiness,
    /// Semantic event body is invalid for the current state.
    Event,
    /// Egress packet body or identity is invalid.
    Egress,
}

/// Bounded child-output failure retaining both protocol stage and boundary class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleNetworkPollError {
    stage: ConsoleNetworkPollStage,
    boundary: BoundaryError,
}

impl ConsoleNetworkPollError {
    const fn new(stage: ConsoleNetworkPollStage, boundary: BoundaryError) -> Self {
        Self { stage, boundary }
    }

    /// Stable containment reason without formatting or allocation on the fault path.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match (self.stage, self.boundary) {
            (ConsoleNetworkPollStage::State, _) => "child-output-state",
            (ConsoleNetworkPollStage::Badge, _) => "child-output-badge",
            (ConsoleNetworkPollStage::CompletionWatermarks, BoundaryError::StaleIdentity) => {
                "child-output-watermark-stale"
            }
            (ConsoleNetworkPollStage::CompletionWatermarks, _) => "child-output-watermark-invalid",
            (ConsoleNetworkPollStage::EventReadiness, BoundaryError::StaleIdentity) => {
                "child-output-event-readiness-stale"
            }
            (ConsoleNetworkPollStage::EventReadiness, _) => "child-output-event-readiness-invalid",
            (ConsoleNetworkPollStage::Event, BoundaryError::InvalidState) => {
                "child-output-event-state"
            }
            (ConsoleNetworkPollStage::Event, BoundaryError::StaleIdentity) => {
                "child-output-event-stale"
            }
            (ConsoleNetworkPollStage::Event, _) => "child-output-event-invalid",
            (ConsoleNetworkPollStage::Egress, BoundaryError::StaleIdentity) => {
                "child-output-egress-stale"
            }
            (ConsoleNetworkPollStage::Egress, _) => "child-output-egress-invalid",
        }
    }
}

impl ConsoleNetworkTurn {
    /// Whether at least one child publication was validated and copied.
    #[must_use]
    pub const fn publication_observed(&self) -> bool {
        self.event.is_some() || self.egress.is_some()
    }

    /// Whether the child retired any exact root-owned input this turn.
    #[must_use]
    pub const fn input_progress_observed(&self) -> bool {
        self.input_completions.any()
    }
}

/// Live kernel resources for one exact console-network generation.
pub struct ConsoleNetworkRuntime {
    boundary: ConsoleNetworkBoundary,
    anchor: seL4_CPtr,
    slots: [seL4_CPtr; ROOT_SLOT_COUNT],
    tracker: RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    tcb: seL4_CPtr,
    scheduling_context: seL4_CPtr,
    child_to_root_notification: seL4_CPtr,
    root_wake_caps: [seL4_CPtr; 5],
    publication_ack_owed: bool,
    standard_fault_cap: seL4_CPtr,
    timeout_fault_cap: seL4_CPtr,
    shared_frames: Vec<RamFrame, SHARED_FRAME_COUNT>,
    direct_virtio_layout: Option<DirectVirtioLayout>,
    direct_genet_layout: Option<DirectGenetLayout>,
    direct_genet_root_ptrs: [usize; DIRECT_GENET_SHARED_PAGE_COUNT],
    direct_genet_armed: bool,
    direct_genet_child_unmapped: u64,
    direct_genet_caps_deleted: u64,
    direct_device_cap: seL4_CPtr,
    direct_device_child_unmapped: bool,
    direct_device_root_mapping: Option<RamFrame>,
    direct_device_deleted: bool,
    direct_irq_handler_cap: seL4_CPtr,
    direct_irq_notification_cap: seL4_CPtr,
    direct_dma_child_unmapped: u64,
    direct_dma_root_mapping: Option<(usize, RamFrame)>,
    entry: usize,
    stack_top: usize,
    init_vaddr: usize,
    descriptor_finalized: bool,
    activated: bool,
    containment_started: bool,
    contained: bool,
    containment: ConsoleNetworkContainmentCursor,
    direct_genet_yield_accounting: DirectGenetYieldAccounting,
}

impl ConsoleNetworkRuntime {
    /// Exact generation currently represented by this handle.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.boundary.generation()
    }

    /// Root policy boundary for lifecycle and authenticated command admission.
    #[must_use]
    pub const fn boundary(&self) -> &ConsoleNetworkBoundary {
        &self.boundary
    }

    /// Whether root may publish one copied ingress packet now.
    #[must_use]
    pub const fn ingress_available(&self) -> bool {
        !self.direct_data_plane()
            && self.activated
            && !self.contained
            && self.boundary.ingress_available()
    }

    /// Whether root may publish one authorized control now.
    #[must_use]
    pub const fn control_available(&self) -> bool {
        self.activated && !self.contained && self.boundary.control_available()
    }

    /// Whether the child has been resumed and not yet contained.
    #[must_use]
    pub const fn activated(&self) -> bool {
        self.activated && !self.contained
    }

    /// Whether the child owns the admitted QEMU VirtIO data path directly.
    #[must_use]
    pub const fn direct_virtio(&self) -> bool {
        self.direct_virtio_layout.is_some()
    }

    /// Whether the child owns one endpoint of the admitted Pi GENET data link.
    #[must_use]
    pub const fn direct_genet(&self) -> bool {
        self.direct_genet_layout.is_some()
    }

    /// Whether packet traffic bypasses the root-owned copied-page adapter.
    #[must_use]
    pub const fn direct_data_plane(&self) -> bool {
        self.direct_virtio() || self.direct_genet()
    }

    /// Acquire one exact child-owned direct-GENET diagnostic publication.
    ///
    /// The CPU-only control page is normal cacheable memory shared between
    /// coherent Pi cores. Root samples only atomic 64-bit words, verifies the
    /// sequence-last commit on both sides of the copy, and rejects any stale,
    /// torn, or cross-generation record. This grants no MMIO or packet-ring
    /// authority to root.
    #[must_use]
    pub(crate) fn direct_genet_runtime_diagnostic(&self) -> Option<DirectGenetRuntimeDiagnostic> {
        if !self.direct_genet() || !self.direct_genet_armed || !self.activated || self.contained {
            return None;
        }
        sample_direct_genet_runtime_diagnostic(self.direct_genet_root_ptrs[0], self.generation())
    }

    /// Whether the immutable ABI-v6 descriptor and initial registers are ready.
    #[must_use]
    pub const fn descriptor_finalized(&self) -> bool {
        self.descriptor_finalized
    }

    /// Exact TCB capability used by bounded bootstrap/handoff diagnostics.
    #[must_use]
    pub(crate) const fn tcb_cptr(&self) -> seL4_CPtr {
        self.tcb
    }

    /// Initialize the post-DHCP CPU-only GENET link exactly once.
    ///
    /// The console child remains suspended. Callers must first prove that the
    /// bootstrap GENET command transport has no active payload command; this
    /// operation intentionally scrubs the pages formerly used for bootstrap.
    pub fn arm_direct_genet(&mut self) -> Result<(), HalError> {
        if !self.direct_genet()
            || self.direct_genet_armed
            || self.descriptor_finalized
            || self.activated
            || self.contained
        {
            return Err(HalError::Unsupported(
                "console-network-direct-genet-arm-state",
            ));
        }
        for index in 1..DIRECT_GENET_SHARED_PAGE_COUNT {
            let root_ptr = self.direct_genet_root_ptrs[index];
            if root_ptr == 0 || !root_ptr.is_multiple_of(SHARED_PAGE_BYTES) {
                return Err(HalError::Unsupported(
                    "console-network-direct-genet-root-alias",
                ));
            }
            // SAFETY: Construction retained one page-aligned root alias for
            // each exact CPU-only GENET shared frame. The caller has fenced the
            // bootstrap command transport, the console child is suspended, and
            // the GENET child has not accepted the direct-link handoff. Thus
            // root is the sole writer for this complete 4 KiB page here.
            let page =
                unsafe { core::slice::from_raw_parts_mut(root_ptr as *mut u8, SHARED_PAGE_BYTES) };
            DirectGenetSlotPage::initialize_into(page)
                .map_err(|_| HalError::Unsupported("console-network-direct-genet-slot-init"))?;
        }
        let control_ptr = self.direct_genet_root_ptrs[0];
        if control_ptr == 0 || !control_ptr.is_multiple_of(SHARED_PAGE_BYTES) {
            return Err(HalError::Unsupported(
                "console-network-direct-genet-root-alias",
            ));
        }
        // SAFETY: This is the control-page member of the same exact, quiescent
        // CPU-only frame population justified above. Initializing it last is
        // the generation publication boundary for both child endpoints.
        let control =
            unsafe { core::slice::from_raw_parts_mut(control_ptr as *mut u8, SHARED_PAGE_BYTES) };
        DirectGenetControlPage::initialize_into(control, self.generation())
            .map_err(|_| HalError::Unsupported("console-network-direct-genet-control-init"))?;
        fence(Ordering::Release);
        self.direct_genet_armed = true;
        Ok(())
    }

    /// Install the immutable ABI-v6 descriptor after physical address acquisition.
    ///
    /// The child remains suspended throughout this operation. The root creates
    /// one temporary writable alias of the already read-only child init frame,
    /// writes and cleans the descriptor, removes that alias, and only then
    /// commits the initial register state. No device capability or mutable
    /// descriptor authority enters the child.
    #[allow(clippy::too_many_arguments)]
    pub fn finalize_descriptor(
        &mut self,
        hal: &mut KernelHal<'_>,
        mac: [u8; 6],
        ipv4: [u8; 4],
        prefix_len: u8,
        gateway: [u8; 4],
        auth_token: &str,
    ) -> Result<(), HalError> {
        if self.descriptor_finalized || self.activated || self.contained {
            return Err(HalError::Unsupported(
                "console-network-descriptor-finalization-state",
            ));
        }
        let contract = ConsoleNetworkContract::from_generated()
            .map_err(|_| HalError::Unsupported("console-network-generated-contract"))?;
        let mut descriptor = contract
            .runtime_init(
                self.generation(),
                mac,
                ipv4,
                prefix_len,
                gateway,
                auth_token,
            )
            .map_err(|_| HalError::Unsupported("console-network-runtime-init"))?;
        if self.direct_genet() && !self.direct_genet_armed {
            return Err(HalError::Unsupported(
                "console-network-direct-genet-not-armed",
            ));
        }
        if self.direct_virtio_layout.is_some() {
            descriptor = descriptor.with_direct_virtio();
        }
        if self.direct_genet_layout.is_some() {
            descriptor = descriptor.with_direct_genet();
        }
        let mut begin_line = heapless::String::<224>::new();
        let _ = core::fmt::write(
            &mut begin_line,
            format_args!(
                "CONSOLE_NETWORK_DESCRIPTOR phase=finalize-begin tcb=0x{:04x} init_frame=0x{:04x} state=suspended abi=v6 direct_virtio={} direct_genet={}",
                self.tcb,
                self.slots[FRAME_SLOT_START + INIT_FRAME_INDEX],
                self.direct_virtio(),
                self.direct_genet(),
            ),
        );
        crate::bootstrap::log::force_uart_line(begin_line.as_str());

        let root_cnode = hal.env.init_cnode_cap();
        let root_depth = sel4::word_bits() as u8;
        let alias = hal.env.try_allocate_slot().map_err(HalError::Sel4)?;
        let init_frame = self.slots[FRAME_SLOT_START + INIT_FRAME_INDEX];
        let copy_error = sel4::cnode_copy_depth(
            root_cnode,
            alias,
            root_depth,
            root_cnode,
            init_frame,
            root_depth,
            sel4_sys::seL4_CapRights_ReadWrite,
        );
        if copy_error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(copy_error));
        }
        let mapped = hal
            .env
            .map_revoke_anchor_frame_in_root(alias, runtime_cacheable_xn_attributes());
        let mut frame = match mapped {
            Ok(frame) => frame,
            Err(error) => {
                let _ = sel4::cnode_delete(root_cnode, alias, root_depth);
                return Err(HalError::Sel4(error));
            }
        };
        frame.as_mut_slice().fill(0);
        let mut encoded = [0u8; RUNTIME_INIT_DESCRIPTOR_BYTES];
        let write_result = descriptor
            .encode(&mut encoded)
            .map_err(|_| HalError::Unsupported("console-network-init-encode"))
            .and_then(|_| {
                frame.as_mut_slice()[..encoded.len()].copy_from_slice(&encoded);
                if let Some(layout) = self.direct_virtio_layout {
                    let mut encoded_layout = [0u8; DIRECT_VIRTIO_LAYOUT_BYTES];
                    layout.encode(&mut encoded_layout).map_err(|_| {
                        HalError::Unsupported("console-network-direct-layout-encode")
                    })?;
                    let end = DIRECT_VIRTIO_LAYOUT_OFFSET + encoded_layout.len();
                    frame.as_mut_slice()[DIRECT_VIRTIO_LAYOUT_OFFSET..end]
                        .copy_from_slice(&encoded_layout);
                }
                if let Some(layout) = self.direct_genet_layout {
                    descriptor
                        .validate_direct_genet_layout(layout)
                        .map_err(|_| {
                            HalError::Unsupported("console-network-direct-genet-layout")
                        })?;
                    let mut encoded_layout = [0u8; DIRECT_GENET_LAYOUT_BYTES];
                    layout.encode(&mut encoded_layout).map_err(|_| {
                        HalError::Unsupported("console-network-direct-genet-layout-encode")
                    })?;
                    let end = DIRECT_GENET_LAYOUT_OFFSET + encoded_layout.len();
                    frame.as_mut_slice()[DIRECT_GENET_LAYOUT_OFFSET..end]
                        .copy_from_slice(&encoded_layout);
                }
                super::cache::cache_clean(
                    sel4_sys::seL4_CapInitThreadVSpace,
                    frame.ptr().as_ptr() as usize,
                    SHARED_PAGE_BYTES,
                )
                .map_err(|error| HalError::Sel4(error.code()))
            });
        let unmap_result = hal.env.unmap_page_cap(alias).map_err(HalError::Sel4);
        let delete_error = sel4::cnode_delete(root_cnode, alias, root_depth);
        write_result?;
        unmap_result?;
        if delete_error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(delete_error));
        }

        sel4::write_tcb_registers(
            self.tcb,
            self.entry,
            self.stack_top,
            seL4_Word::try_from(self.init_vaddr)
                .map_err(|_| HalError::Unsupported("console-network-init-arg"))?,
            false,
        )
        .map_err(HalError::Sel4)?;
        self.descriptor_finalized = true;
        let mut ready_line = heapless::String::<224>::new();
        let _ = core::fmt::write(
            &mut ready_line,
            format_args!(
                "CONSOLE_NETWORK_DESCRIPTOR phase=finalize-ready tcb=0x{:04x} init_frame=0x{:04x} root_alias=0x{:04x} state=suspended root_alias_state=deleted abi=v6 direct_virtio={} direct_genet={}",
                self.tcb, init_frame, alias, self.direct_virtio(), self.direct_genet(),
            ),
        );
        crate::bootstrap::log::force_uart_line(ready_line.as_str());
        Ok(())
    }

    /// Whether the exact terminal generation has entered containment.
    #[must_use]
    pub const fn containment_active(&self) -> bool {
        self.containment_started
    }

    /// Fence root admission and retain all kernel resources for later units.
    pub fn begin_containment(&mut self) -> Result<(), HalError> {
        if self.contained || self.containment_active() {
            return Err(HalError::Unsupported("console-network-containment-state"));
        }
        self.boundary.record_fault();
        // Containment abandons any copied publication without granting the
        // child new publication authority.
        self.publication_ack_owed = false;
        self.activated = false;
        self.containment_started = true;
        Ok(())
    }

    /// Mark a critical-lane fault before root performs complete containment.
    pub fn record_supervisor_fault(&mut self) {
        self.boundary.record_fault();
        self.publication_ack_owed = false;
    }

    /// Resume the child after the target fault registry is sealed.
    pub fn activate(&mut self) -> Result<(), HalError> {
        if !self.descriptor_finalized {
            return Err(HalError::Unsupported(
                "console-network-descriptor-not-finalized",
            ));
        }
        if self.activated || self.contained {
            return Err(HalError::Unsupported("console-network-activation-state"));
        }
        sel4::resume_tcb(self.tcb).map_err(HalError::Sel4)?;
        self.activated = true;
        Ok(())
    }

    fn committed_signal_yield_admitted(
        &self,
        durable_publication: bool,
        signal_badge: u64,
    ) -> Result<bool, BoundaryError> {
        let contract = self.boundary.contract();
        if contract.cross_core_signal_only {
            if !console_network_signal_only_admitted(ConsoleNetworkSignalOnlyAdmission {
                contract_direct_genet: contract.direct_genet,
                runtime_direct_genet: self.direct_genet(),
                cross_core_signal_only: contract.cross_core_signal_only,
                yield_to_child_after_signal: contract.yield_to_child_after_signal,
                activated: self.activated,
                containment_started: self.containment_started,
                contained: self.contained,
                scheduling_context_present: self.scheduling_context != sel4_sys::seL4_CapNull,
            }) {
                return Err(BoundaryError::HandoffFailed);
            }
            return Ok(false);
        }
        if contract.yield_to_child_after_signal && !contract.direct_genet {
            return Err(BoundaryError::HandoffFailed);
        }
        let admission = ConsoleNetworkYieldToAdmission {
            profile_enabled: contract.yield_to_child_after_signal,
            runtime_direct_virtio: self.direct_virtio(),
            runtime_direct_genet: self.direct_genet(),
            service_state: self.boundary.state(),
            durable_publication,
            signal_badge,
            activated: self.activated,
            containment_started: self.containment_started,
            contained: self.contained,
            scheduling_context_present: self.scheduling_context != sel4_sys::seL4_CapNull,
        };
        let exact_authenticated_direct_genet_work = contract.yield_to_child_after_signal
            && !admission.runtime_direct_virtio
            && admission.runtime_direct_genet
            && matches!(admission.service_state, ServiceState::Authenticated)
            && admission.durable_publication
            && admission.signal_badge.is_power_of_two()
            && matches!(admission.signal_badge, WAKE_CONTROL | WAKE_PUBLICATION_ACK);
        if !exact_authenticated_direct_genet_work {
            return Ok(false);
        }
        if !console_network_yield_to_admitted(admission) {
            return Err(BoundaryError::HandoffFailed);
        }
        Ok(true)
    }

    fn yield_to_child_after_committed_signal(
        &mut self,
        yield_admitted: bool,
        predrain_succeeded: bool,
    ) -> Result<(), BoundaryError> {
        if !direct_genet_yield_after_predrain_admitted(yield_admitted, predrain_succeeded) {
            return Ok(());
        }
        #[cfg(all(
            feature = "kernel",
            feature = "release-pi4",
            feature = "net-backend-genet-direct",
            target_arch = "aarch64",
            target_os = "none",
            sel4_config_kernel_mcs
        ))]
        {
            let counter_hz = crate::arch::aarch64::timer::timer_freq_hz();
            let started_ticks = crate::arch::aarch64::timer::timer_counter_ticks();
            let yielded = sel4::yield_to_sched_context(self.scheduling_context);
            let finished_ticks = crate::arch::aarch64::timer::timer_counter_ticks();
            self.direct_genet_yield_accounting.record_call(
                predrain_succeeded,
                counter_hz,
                started_ticks,
                finished_ticks,
                yielded.as_ref().ok().copied(),
            );
            yielded
                .map(|_| ())
                .map_err(|_| BoundaryError::HandoffFailed)
        }
        #[cfg(not(all(
            feature = "kernel",
            feature = "release-pi4",
            feature = "net-backend-genet-direct",
            target_arch = "aarch64",
            target_os = "none",
            sel4_config_kernel_mcs
        )))]
        {
            let _ = predrain_succeeded;
            Err(BoundaryError::HandoffFailed)
        }
    }

    fn signal_committed_child_work(
        &mut self,
        wake_index: usize,
        publication: DurableChildPublication,
    ) -> Result<(), BoundaryError> {
        let wake_cap = self
            .root_wake_caps
            .get(wake_index)
            .copied()
            .filter(|cap| *cap != sel4_sys::seL4_CapNull)
            .ok_or(BoundaryError::HandoffFailed)?;
        let signal_badge = root_wake_badge(wake_index)
            .filter(|badge| badge.is_power_of_two())
            .ok_or(BoundaryError::HandoffFailed)?;
        let durable_publication = publication.committed_for_badge(signal_badge)?;
        let yield_admitted =
            self.committed_signal_yield_admitted(durable_publication, signal_badge)?;
        #[cfg(all(
            feature = "kernel",
            feature = "release-pi4",
            feature = "net-backend-genet-direct",
            target_arch = "aarch64",
            target_os = "none",
            sel4_config_kernel_mcs
        ))]
        let predrain_succeeded = {
            if direct_genet_predrain_required(self.direct_genet(), yield_admitted) {
                let succeeded = sel4::sched_context_consumed(self.scheduling_context).is_ok();
                if !succeeded {
                    self.direct_genet_yield_accounting
                        .invalidate(DIRECT_GENET_YIELD_ACCOUNTING_INVALID_PREDRAIN);
                }
                succeeded
            } else {
                true
            }
        };
        #[cfg(not(all(
            feature = "kernel",
            feature = "release-pi4",
            feature = "net-backend-genet-direct",
            target_arch = "aarch64",
            target_os = "none",
            sel4_config_kernel_mcs
        )))]
        let predrain_succeeded = true;
        // The shared-page or publication-credit commit precedes this release.
        // The selected cross-core Pi profile stops after its one-hot signal;
        // only the retired same-core comparison path can admit YieldTo.
        fence(Ordering::Release);
        sel4::signal_unchecked(wake_cap);
        self.yield_to_child_after_committed_signal(yield_admitted, predrain_succeeded)
    }

    /// Return fail-closed accounting for exact same-core direct-GENET YieldTo.
    #[must_use]
    pub const fn direct_genet_yield_accounting(&self) -> DirectGenetYieldAccounting {
        self.direct_genet_yield_accounting
    }

    /// Bracket a physical TCP identity through the SCs already owned by each
    /// supervisor. This runs at observed lifecycle edges, never per packet.
    /// A delayed or fault-preempted driver sample remains explicitly missing.
    #[cfg(all(feature = "release-pi4", sel4_config_kernel_mcs))]
    pub(crate) fn sample_tcp_session_consumed(&self, connection: u64, finish: bool) {
        use crate::pi4_mcs_consumed::{self as accounting, Role, Sample};
        let contract = self.boundary.contract();
        if !self.activated
            || self.containment_started
            || self.contained
            || self.direct_virtio()
            || !contract.cross_core_signal_only
            || contract.yield_to_child_after_signal
        {
            return;
        }
        let Some(request) =
            accounting::request(self.generation(), connection, self.direct_genet(), finish)
        else {
            return;
        };
        let entered = crate::arch::aarch64::timer::timer_counter_ticks();
        let result = super::critical_tcb::root_control_consumed_time_us();
        let returned = crate::arch::aarch64::timer::timer_counter_ticks();
        accounting::store(
            request,
            Role::Root,
            Sample::capture(Role::Root, 1, entered, returned, result.is_ok()),
        );
        let entered = crate::arch::aarch64::timer::timer_counter_ticks();
        let result = sel4::sched_context_consumed(self.scheduling_context);
        let returned = crate::arch::aarch64::timer::timer_counter_ticks();
        accounting::record_drain(Role::Console, result.ok());
        accounting::store(
            request,
            Role::Console,
            Sample::capture(
                Role::Console,
                self.generation(),
                entered,
                returned,
                result.is_ok(),
            ),
        );
        super::critical_tcb::signal_driver_accounting_request();
    }

    /// Stage one virtual/admitted NIC packet and signal its exact one-hot wake.
    pub fn stage_ingress(&mut self, packet: &[u8]) -> Result<u64, BoundaryError> {
        if self.direct_data_plane() || !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        let sequence = self
            .boundary
            .stage_ingress(packet, self.shared_frames[0].as_mut_slice())?;
        self.signal_committed_child_work(ROOT_PACKET_RX_WAKE_INDEX, DurableChildPublication::None)?;
        Ok(sequence)
    }

    /// Stage one root-authorized response, preserving its exact bytes.
    pub fn stage_authorized_line(&mut self, line: &str, now_ms: u64) -> Result<u64, BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        let sequence = self.boundary.stage_authorized_line(
            line,
            now_ms,
            self.shared_frames[2].as_mut_slice(),
        )?;
        self.signal_committed_child_work(
            ROOT_CONTROL_WAKE_INDEX,
            DurableChildPublication::AuthenticatedControl,
        )?;
        Ok(sequence)
    }

    /// Stage one bounded response batch and signal its exact control wake.
    pub fn stage_authorized_batch(
        &mut self,
        payload: &[u8],
        now_ms: u64,
    ) -> Result<u64, BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        let sequence = self.boundary.stage_authorized_batch(
            payload,
            now_ms,
            self.shared_frames[2].as_mut_slice(),
        )?;
        self.signal_committed_child_work(
            ROOT_CONTROL_WAKE_INDEX,
            DurableChildPublication::AuthenticatedControl,
        )?;
        Ok(sequence)
    }

    /// Stop command admission and request close-after-flush.
    pub fn stage_disconnect(&mut self, now_ms: u64) -> Result<u64, BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        let sequence = self
            .boundary
            .stage_disconnect(now_ms, self.shared_frames[2].as_mut_slice())?;
        self.signal_committed_child_work(ROOT_CONTROL_WAKE_INDEX, DurableChildPublication::None)?;
        Ok(sequence)
    }

    /// Wake one bounded child turn without publishing a new control record.
    pub fn service_tick(&mut self) -> Result<(), BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        self.signal_committed_child_work(ROOT_CONTROL_WAKE_INDEX, DurableChildPublication::None)
    }

    /// Whether the newest response for this connection left the TCP send queue.
    #[must_use]
    pub fn console_output_drained(&self, connection_id: u64) -> bool {
        self.boundary.console_output_drained(connection_id)
    }

    /// Peek whether the child has already committed any publication that root
    /// has not consumed yet.
    ///
    /// This level read is the final stale-edge guard before root-control waits
    /// on the shared fan-in. It neither consumes the child notification nor
    /// accepts the shared record; the ordinary isolated-console rotor retains
    /// sole publication ownership.
    pub fn child_publication_pending(&self) -> Result<bool, BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        self.boundary.child_publication_pending(
            self.shared_frames[3].as_slice(),
            self.shared_frames[1].as_slice(),
        )
    }

    /// Return the exact root control record whose child watermark is still
    /// causally owed. This does not accept or advance either shared page.
    #[must_use]
    pub fn child_control_publication_owed(
        &self,
    ) -> Option<crate::console_network_service::ConsoleNetworkControlPublication> {
        (self.activated && !self.contained)
            .then(|| self.boundary.control_publication_owed())
            .flatten()
    }

    /// Close admission and signal the bounded shutdown path.
    pub fn begin_shutdown(&mut self) -> Result<(), BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        self.boundary.begin_shutdown()?;
        fence(Ordering::Release);
        sel4::signal_unchecked(self.root_wake_caps[ROOT_SHUTDOWN_WAKE_INDEX]);
        Ok(())
    }

    /// Signal immediate revoke. Root still suspends and revokes kernel objects.
    pub fn signal_revoke(&mut self) -> Result<(), BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        self.boundary.record_fault();
        // The outstanding page is intentionally abandoned once revoke wins;
        // never turn containment into a fresh publication credit.
        self.publication_ack_owed = false;
        fence(Ordering::Release);
        sel4::signal_unchecked(self.root_wake_caps[ROOT_REVOKE_WAKE_INDEX]);
        Ok(())
    }

    /// Whether one copied child publication still needs its one-shot ACK.
    #[must_use]
    pub const fn publication_ack_pending(&self) -> bool {
        self.activated && !self.contained && self.publication_ack_owed
    }

    /// Poll and validate all coalesced child output bits without blocking root.
    pub fn poll_turn(&mut self) -> Result<ConsoleNetworkTurn, ConsoleNetworkPollError> {
        if !self.activated || self.contained || self.publication_ack_owed {
            return Err(ConsoleNetworkPollError::new(
                ConsoleNetworkPollStage::State,
                BoundaryError::InvalidState,
            ));
        }
        let mut badge = 0;
        let _ = sel4::poll(self.child_to_root_notification, &mut badge);
        if badge == 0 {
            return Ok(ConsoleNetworkTurn {
                input_completions:
                    crate::console_network_service::ConsoleNetworkInputCompletions::default(),
                event: None,
                egress: None,
            });
        }
        if badge & !(CHILD_WAKE_MASK as seL4_Word) != 0 {
            self.boundary.record_fault();
            return Err(ConsoleNetworkPollError::new(
                ConsoleNetworkPollStage::Badge,
                BoundaryError::InvalidRecord,
            ));
        }
        fence(Ordering::Acquire);
        let event_ready = badge & seL4_Word::from(WAKE_EVENT_READY) != 0;
        let input_completions = if event_ready {
            self.boundary
                .accept_completion_watermarks(self.shared_frames[3].as_slice())
                .map_err(|boundary| {
                    ConsoleNetworkPollError::new(
                        ConsoleNetworkPollStage::CompletionWatermarks,
                        boundary,
                    )
                })?
        } else {
            crate::console_network_service::ConsoleNetworkInputCompletions::default()
        };
        let event = if event_ready {
            if self
                .boundary
                .event_publication_pending(self.shared_frames[3].as_slice())
                .map_err(|boundary| {
                    ConsoleNetworkPollError::new(ConsoleNetworkPollStage::EventReadiness, boundary)
                })?
            {
                Some(
                    self.boundary
                        .accept_event(self.shared_frames[3].as_slice())
                        .map_err(|boundary| {
                            ConsoleNetworkPollError::new(ConsoleNetworkPollStage::Event, boundary)
                        })?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let egress = if badge & seL4_Word::from(WAKE_PACKET_TX_READY) != 0 {
            Some(
                self.boundary
                    .accept_egress(self.shared_frames[1].as_slice())
                    .map_err(|boundary| {
                        ConsoleNetworkPollError::new(ConsoleNetworkPollStage::Egress, boundary)
                    })?,
            )
        } else {
            None
        };
        // A coalesced event+egress observation still earns one global credit:
        // both records are root-owned before the single ACK is made available.
        self.publication_ack_owed = event.is_some() || egress.is_some();
        Ok(ConsoleNetworkTurn {
            input_completions,
            event,
            egress,
        })
    }

    fn reset_unmap_direct_device(&mut self, hal: &mut KernelHal<'_>) -> Result<(), HalError> {
        if self.direct_device_cap == sel4_sys::seL4_CapNull || self.direct_device_deleted {
            return Ok(());
        }
        if !self.direct_device_child_unmapped {
            hal.env
                .unmap_page_cap(self.direct_device_cap)
                .map_err(HalError::Sel4)?;
            self.direct_device_child_unmapped = true;
        }
        if self.direct_device_root_mapping.is_none() {
            let mapping = hal
                .env
                .map_revoke_anchor_frame_in_root(
                    self.direct_device_cap,
                    runtime_uncached_xn_attributes(),
                )
                .map_err(HalError::Sel4)?;
            self.direct_device_root_mapping = Some(mapping);
        }
        let mapping = self
            .direct_device_root_mapping
            .as_ref()
            .ok_or(HalError::Unsupported(
                "console-network-direct-device-mapping",
            ))?;
        let status = mapping.ptr().as_ptr().wrapping_add(0x70).cast::<u32>();
        // SAFETY: The child is suspended and the sole admitted VirtIO MMIO cap
        // is now mapped only in root. Offset 0x70 is the aligned device-status
        // register within that validated page. Writing zero synchronously
        // resets queue DMA before any DMA page is scrubbed.
        unsafe {
            write_volatile(status, 0);
            fence(Ordering::SeqCst);
            if read_volatile(status) != 0 {
                return Err(HalError::Unsupported("console-network-direct-device-reset"));
            }
        }
        hal.env
            .unmap_page_cap(self.direct_device_cap)
            .map_err(HalError::Sel4)?;
        self.direct_device_root_mapping = None;
        let error = sel4::cnode_delete_bounded(
            hal.env.init_cnode_cap(),
            self.direct_device_cap,
            sel4::word_bits() as u8,
        );
        if error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(error));
        }
        self.direct_device_deleted = true;
        Ok(())
    }

    #[cfg(feature = "net-backend-virtio")]
    fn scrub_direct_dma_frame(
        &mut self,
        frame_index: usize,
        hal: &mut KernelHal<'_>,
    ) -> Result<(), HalError> {
        if frame_index >= DIRECT_DMA_FRAME_COUNT {
            return Err(HalError::Unsupported("console-network-direct-frame-index"));
        }
        if self
            .direct_dma_root_mapping
            .as_ref()
            .is_some_and(|(mapped_index, _)| *mapped_index != frame_index)
        {
            return Err(HalError::Unsupported("console-network-direct-frame-order"));
        }
        let frame_cap = self.slots[FRAME_SLOT_START + DIRECT_FRAME_START + frame_index];
        let bit = 1u64 << frame_index;
        if self.direct_dma_root_mapping.is_none() {
            if self.direct_dma_child_unmapped & bit == 0 {
                hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
                self.direct_dma_child_unmapped |= bit;
            }
            let mapping = hal
                .env
                .map_revoke_anchor_frame_in_root(frame_cap, direct_virtio_dma_attributes())
                .map_err(HalError::Sel4)?;
            self.direct_dma_root_mapping = Some((frame_index, mapping));
        }
        let frame = self
            .direct_dma_root_mapping
            .as_mut()
            .map(|(_, frame)| frame)
            .ok_or(HalError::Unsupported(
                "console-network-direct-frame-mapping",
            ))?;
        frame.as_mut_slice().fill(0);
        super::cache::cache_clean_bounded(
            sel4_sys::seL4_CapInitThreadVSpace,
            frame.ptr().as_ptr() as usize,
            DIRECT_VIRTIO_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
        self.direct_dma_root_mapping = None;
        Ok(())
    }

    #[cfg(not(feature = "net-backend-virtio"))]
    fn scrub_direct_dma_frame(
        &mut self,
        _frame_index: usize,
        _hal: &mut KernelHal<'_>,
    ) -> Result<(), HalError> {
        // The non-VirtIO containment cursor has zero direct frames and cannot
        // select this unit. Reject a corrupted cursor instead of compiling a
        // QEMU DMA mapping path into a physical-network profile.
        Err(HalError::Unsupported(
            "console-network-direct-virtio-disabled",
        ))
    }

    /// Grant one publication credit after the adapter retained all copied output.
    pub fn acknowledge_publication(&mut self) -> Result<(), BoundaryError> {
        if !self.activated || self.contained || !self.publication_ack_owed {
            return Err(BoundaryError::InvalidState);
        }
        // Consume root's one-shot authority before signalling. Even a later
        // erroneous duplicate call cannot credit an unobserved replacement.
        self.publication_ack_owed = false;
        self.signal_committed_child_work(
            ROOT_PUBLICATION_ACK_WAKE_INDEX,
            DurableChildPublication::AuthenticatedPublicationCredit,
        )
    }

    /// Retire a validated terminal publication without waking the parked child.
    pub fn retire_terminal_publication(&mut self) -> Result<(), BoundaryError> {
        if !self.activated
            || self.contained
            || !self.publication_ack_owed
            || self.boundary.state() != ServiceState::Terminal
        {
            return Err(BoundaryError::InvalidState);
        }
        // ShutdownComplete consumes the child's last credit and parks it. The
        // root closes its exactly-once latch but deliberately sends no ACK.
        self.publication_ack_owed = false;
        Ok(())
    }

    /// Perform at most one ordered containment unit for this recovery turn.
    pub fn contain_one_turn(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<ConsoleNetworkContainmentTurn, HalError> {
        if !self.containment_active() {
            return Err(HalError::Unsupported(
                "console-network-containment-not-latched",
            ));
        }

        let selected = self.containment.select_next();
        let result = match selected {
            ConsoleNetworkContainmentUnit::SuspendTcb => {
                sel4::suspend_tcb_bounded(self.tcb).map_err(HalError::Sel4)
            }
            ConsoleNetworkContainmentUnit::UnbindSchedulingContext => {
                sel4::unbind_sched_context_object(self.scheduling_context, self.tcb, None)
                    .map_err(HalError::Sel4)
            }
            ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(frame_index) => {
                match self.shared_frames.get_mut(frame_index) {
                    Some(frame) => {
                        frame.as_mut_slice().fill(0);
                        super::cache::cache_clean_bounded(
                            sel4_sys::seL4_CapInitThreadVSpace,
                            frame.ptr().as_ptr() as usize,
                            SHARED_PAGE_BYTES,
                        )
                        .map_err(|error| HalError::Sel4(error.code()))
                    }
                    None => Err(HalError::Unsupported(
                        "console-network-containment-frame-index",
                    )),
                }
            }
            ConsoleNetworkContainmentUnit::UnmapSharedFrame(frame_index) => {
                match self.shared_frames.get(frame_index) {
                    Some(frame) => hal.env.unmap_page_cap(frame.cap()).map_err(HalError::Sel4),
                    None => Err(HalError::Unsupported(
                        "console-network-containment-frame-index",
                    )),
                }
            }
            ConsoleNetworkContainmentUnit::FenceDirectGenetPeer => hal.fence_direct_genet_peer(),
            ConsoleNetworkContainmentUnit::UnmapDirectGenetFrame(frame_index) => {
                if frame_index >= DIRECT_GENET_FRAME_COPY_SLOT_COUNT {
                    Err(HalError::Unsupported(
                        "console-network-direct-genet-frame-index",
                    ))
                } else {
                    let bit = 1u64 << frame_index;
                    if self.direct_genet_child_unmapped & bit != 0 {
                        Err(HalError::Unsupported(
                            "console-network-direct-genet-unmap-order",
                        ))
                    } else {
                        hal.env
                            .unmap_page_cap(
                                self.slots[DIRECT_GENET_FRAME_COPY_SLOT_START + frame_index],
                            )
                            .map_err(HalError::Sel4)
                            .map(|()| self.direct_genet_child_unmapped |= bit)
                    }
                }
            }
            ConsoleNetworkContainmentUnit::DeleteDirectGenetFrameCap(frame_index) => {
                if frame_index >= DIRECT_GENET_FRAME_COPY_SLOT_COUNT {
                    Err(HalError::Unsupported(
                        "console-network-direct-genet-frame-index",
                    ))
                } else {
                    let bit = 1u64 << frame_index;
                    if self.direct_genet_child_unmapped & bit == 0
                        || self.direct_genet_caps_deleted & bit != 0
                    {
                        Err(HalError::Unsupported(
                            "console-network-direct-genet-delete-order",
                        ))
                    } else {
                        let error = sel4::cnode_delete_bounded(
                            hal.env.init_cnode_cap(),
                            self.slots[DIRECT_GENET_FRAME_COPY_SLOT_START + frame_index],
                            sel4::word_bits() as u8,
                        );
                        if error == sel4_sys::seL4_NoError {
                            self.direct_genet_caps_deleted |= bit;
                            Ok(())
                        } else {
                            Err(HalError::Sel4(error))
                        }
                    }
                }
            }
            ConsoleNetworkContainmentUnit::ClearDirectIrq => {
                let error = sel4::irq_handler_clear(self.direct_irq_handler_cap);
                if error == sel4_sys::seL4_NoError {
                    Ok(())
                } else {
                    Err(HalError::Sel4(error))
                }
            }
            ConsoleNetworkContainmentUnit::RevokeDirectIrqHandler => {
                let error = sel4::cnode_revoke(
                    hal.env.init_cnode_cap(),
                    self.direct_irq_handler_cap,
                    sel4::word_bits() as u8,
                );
                if error == sel4_sys::seL4_NoError {
                    Ok(())
                } else {
                    Err(HalError::Sel4(error))
                }
            }
            ConsoleNetworkContainmentUnit::DeleteDirectIrqNotification => {
                let error = sel4::cnode_delete_bounded(
                    hal.env.init_cnode_cap(),
                    self.direct_irq_notification_cap,
                    sel4::word_bits() as u8,
                );
                if error == sel4_sys::seL4_NoError {
                    self.direct_irq_notification_cap = sel4_sys::seL4_CapNull;
                    Ok(())
                } else {
                    Err(HalError::Sel4(error))
                }
            }
            ConsoleNetworkContainmentUnit::DeleteDirectIrqHandler => {
                let error = sel4::cnode_delete_bounded(
                    hal.env.init_cnode_cap(),
                    self.direct_irq_handler_cap,
                    sel4::word_bits() as u8,
                );
                if error == sel4_sys::seL4_NoError {
                    self.direct_irq_handler_cap = sel4_sys::seL4_CapNull;
                    Ok(())
                } else {
                    Err(HalError::Sel4(error))
                }
            }
            ConsoleNetworkContainmentUnit::ResetUnmapDirectDevice => {
                self.reset_unmap_direct_device(hal)
            }
            ConsoleNetworkContainmentUnit::ScrubDirectFrame(frame_index) => {
                self.scrub_direct_dma_frame(frame_index, hal)
            }
            ConsoleNetworkContainmentUnit::DeleteFaultCap(cap_index) => {
                match [self.standard_fault_cap, self.timeout_fault_cap]
                    .get(cap_index)
                    .copied()
                {
                    Some(cap) => {
                        let error = sel4::cnode_delete_bounded(
                            hal.env.init_cnode_cap(),
                            cap,
                            sel4::word_bits() as u8,
                        );
                        if error == sel4_sys::seL4_NoError {
                            Ok(())
                        } else {
                            Err(HalError::Sel4(error))
                        }
                    }
                    None => Err(HalError::Unsupported(
                        "console-network-containment-fault-cap-index",
                    )),
                }
            }
            ConsoleNetworkContainmentUnit::RevokeAnchor => hal
                .env
                .revoke_anchor_descendants_and_reset_vspace(self.anchor, &mut self.tracker)
                .map_err(|error| match error {
                    sel4::RevokeAnchorVSpaceError::Sel4(error) => HalError::Sel4(error),
                    _ => HalError::Unsupported("console-network-revoke-tracker"),
                }),
            ConsoleNetworkContainmentUnit::Finalize | ConsoleNetworkContainmentUnit::Complete => {
                Ok(())
            }
        };
        if let Err(error) = result {
            self.containment.restore_selected(selected);
            return Err(error);
        }
        if selected != ConsoleNetworkContainmentUnit::Complete {
            return Ok(ConsoleNetworkContainmentTurn::InProgress);
        }

        self.contained = true;
        self.activated = false;
        Ok(ConsoleNetworkContainmentTurn::Complete(
            ConsoleNetworkContainmentProof {
                tcb_suspended: true,
                scheduling_context_unbound: true,
                mappings_scrubbed: true,
                capabilities_revoked: true,
                objects_deleted: true,
                generation_fenced: true,
            },
        ))
    }

    /// Root slots permanently reserved for deterministic generation reuse.
    #[must_use]
    pub const fn reserved_root_slots(&self) -> &[seL4_CPtr; ROOT_SLOT_COUNT] {
        &self.slots
    }
}

impl<'a> KernelHal<'a> {
    /// Construct the exact child generation without an address descriptor.
    ///
    /// This is the physical-Pi pre-seal phase: every object, MCS binding, and
    /// fault identity exists, but the TCB is suspended and has no initial
    /// registers until DHCP truth is available.
    pub fn construct_console_network_runtime_shell(
        &mut self,
        generation: u64,
    ) -> Result<ConsoleNetworkRuntime, HalError> {
        self.construct_console_network_runtime_shell_selected(generation, false)
    }

    /// Construct the wired-Pi shell with the compiler-declared direct GENET
    /// CPU-page link. Wi-Fi callers must use the ordinary shell above and never
    /// consume or map GENET resources.
    pub fn construct_direct_genet_console_network_runtime_shell(
        &mut self,
        generation: u64,
    ) -> Result<ConsoleNetworkRuntime, HalError> {
        self.construct_console_network_runtime_shell_selected(generation, true)
    }

    fn construct_console_network_runtime_shell_selected(
        &mut self,
        generation: u64,
        direct_genet: bool,
    ) -> Result<ConsoleNetworkRuntime, HalError> {
        let mut begin_line = heapless::String::<128>::new();
        let _ = core::fmt::write(
            &mut begin_line,
            format_args!(
                "CONSOLE_NETWORK_SHELL phase=begin generation={} state=suspended descriptor=pending",
                generation,
            ),
        );
        crate::bootstrap::log::force_uart_line(begin_line.as_str());
        let contract = ConsoleNetworkContract::from_generated()
            .map_err(|_| HalError::Unsupported("console-network-generated-contract"))?;
        let object_plan = ConsoleNetworkObjectPlan::from_generated()
            .map_err(|_| HalError::Unsupported("console-network-object-plan"))?;
        validate_object_plan(object_plan)?;

        let anchor = self
            .env
            .create_revoke_anchor(
                seL4_CPtr::from(object_plan.revoke_anchor_slot),
                object_plan.revoke_anchor_bits,
            )
            .map_err(HalError::Sel4)?;
        let mut slots = [sel4_sys::seL4_CapNull; ROOT_SLOT_COUNT];
        for slot in &mut slots {
            match self.env.try_allocate_slot() {
                Ok(cap) => *slot = cap,
                Err(error) => {
                    let _ = sel4::cnode_revoke(
                        self.env.init_cnode_cap(),
                        anchor,
                        sel4::word_bits() as u8,
                    );
                    return Err(HalError::Sel4(error));
                }
            }
        }
        let mut slots_line = heapless::String::<224>::new();
        let _ = core::fmt::write(
            &mut slots_line,
            format_args!(
                "CONSOLE_NETWORK_SHELL phase=root-slots-ready generation={} anchor=0x{:04x} first=0x{:04x} last=0x{:04x} count={}",
                generation,
                anchor,
                slots[0],
                slots[ROOT_SLOT_COUNT - 1],
                ROOT_SLOT_COUNT,
            ),
        );
        crate::bootstrap::log::force_uart_line(slots_line.as_str());
        let translation_slots: [seL4_CPtr; TRANSLATION_SLOT_COUNT] = slots
            [TRANSLATION_SLOT_START..TRANSLATION_SLOT_START + TRANSLATION_SLOT_COUNT]
            .try_into()
            .map_err(|_| HalError::Unsupported("console-network-translation-slots"))?;
        let mut tracker = RevokeAnchorVSpaceTracker::new(translation_slots)
            .map_err(|_| HalError::Unsupported("console-network-translation-tracker"))?;

        let result = construct_generation(
            self,
            contract,
            object_plan,
            generation,
            anchor,
            slots,
            &mut tracker,
            direct_genet,
        );
        if result.is_err() {
            let root_cnode = self.env.init_cnode_cap();
            let root_depth = sel4::word_bits() as u8;
            #[cfg(feature = "net-backend-genet-direct")]
            for slot in &slots[DIRECT_GENET_FRAME_COPY_SLOT_START
                ..DIRECT_GENET_FRAME_COPY_SLOT_START + DIRECT_GENET_FRAME_COPY_SLOT_COUNT]
            {
                let _ = self.env.unmap_page_cap(*slot);
                let _ = sel4::cnode_delete(root_cnode, *slot, root_depth);
            }
            #[cfg(feature = "net-backend-virtio")]
            {
                let _ = sel4::irq_handler_clear(slots[DIRECT_IRQ_HANDLER_SLOT_INDEX]);
                let _ = sel4::cnode_revoke(
                    root_cnode,
                    slots[DIRECT_IRQ_HANDLER_SLOT_INDEX],
                    root_depth,
                );
                let _ = sel4::cnode_delete(
                    root_cnode,
                    slots[DIRECT_IRQ_NOTIFICATION_SLOT_INDEX],
                    root_depth,
                );
                let _ = sel4::cnode_delete(
                    root_cnode,
                    slots[DIRECT_IRQ_HANDLER_SLOT_INDEX],
                    root_depth,
                );
                let _ = self.env.unmap_page_cap(slots[DIRECT_MMIO_SLOT_INDEX]);
                let _ = sel4::cnode_delete(root_cnode, slots[DIRECT_MMIO_SLOT_INDEX], root_depth);
            }
            let _ = sel4::cnode_delete(root_cnode, slots[STANDARD_FAULT_SLOT_INDEX], root_depth);
            let _ = sel4::cnode_delete(root_cnode, slots[TIMEOUT_FAULT_SLOT_INDEX], root_depth);
            let _ = self
                .env
                .revoke_anchor_descendants_and_reset_vspace(anchor, &mut tracker);
        }
        result
    }

    /// Construct the exact child generation and register its fault source.
    ///
    /// The returned TCB is suspended. Callers must construct every generated
    /// target, seal the critical fault registry, and only then call
    /// [`ConsoleNetworkRuntime::activate`].
    pub fn construct_console_network_runtime(
        &mut self,
        generation: u64,
        mac: [u8; 6],
        ipv4: [u8; 4],
        prefix_len: u8,
        gateway: [u8; 4],
        auth_token: &str,
    ) -> Result<ConsoleNetworkRuntime, HalError> {
        let mut runtime = self.construct_console_network_runtime_shell(generation)?;
        runtime.finalize_descriptor(self, mac, ipv4, prefix_len, gateway, auth_token)?;
        Ok(runtime)
    }
}

fn validate_object_plan(plan: ConsoleNetworkObjectPlan) -> Result<(), HalError> {
    if plan.objects.tcbs != 1
        || plan.objects.cnodes != 1
        || plan.objects.vspaces != 1
        || plan.objects.page_tables as usize != TRANSLATION_SLOT_COUNT
        || plan.objects.asids != 1
        || plan.objects.frames as usize != FRAME_COUNT
        || plan.objects.notifications != 2
        || plan.objects.endpoints != 0
        || plan.objects.scheduling_contexts != 1
        || plan.objects.cspace_slots as usize != ROOT_SLOT_COUNT
        || usize::from(plan.image_pages) != STACK_FRAME_START
    {
        return Err(HalError::Unsupported("console-network-object-plan-drift"));
    }
    Ok(())
}

fn construct_generation(
    hal: &mut KernelHal<'_>,
    contract: ConsoleNetworkContract,
    object_plan: ConsoleNetworkObjectPlan,
    generation: u64,
    anchor: seL4_CPtr,
    slots: [seL4_CPtr; ROOT_SLOT_COUNT],
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    direct_genet: bool,
) -> Result<ConsoleNetworkRuntime, HalError> {
    let boundary = ConsoleNetworkBoundary::new(generation)
        .map_err(|_| HalError::Unsupported("console-network-boundary"))?;
    let stack_top = usize::try_from(contract.stack_vaddr)
        .ok()
        .and_then(|base| base.checked_add(usize::from(contract.stack_pages) << sel4::PAGE_BITS))
        .ok_or(HalError::Unsupported("console-network-stack-top"))?
        & !0xf;
    let init_vaddr = usize::try_from(contract.init_vaddr)
        .map_err(|_| HalError::Unsupported("console-network-init-vaddr"))?;
    if !CONSOLE_NETWORK_IMAGE_IDENTITY_BOUND {
        return Err(HalError::Unsupported("console-network-image-unbound"));
    }
    let image_plan = plan_runtime_elf_load(CONSOLE_NETWORK_RUNTIME_IMAGE, object_plan.image_pages)?;
    let expected_limit = image_plan
        .base_vaddr
        .checked_add(image_plan.page_count << sel4::PAGE_BITS)
        .ok_or(HalError::Unsupported("console-network-image-span"))?;
    if image_plan.entry as u64 != CONSOLE_NETWORK_RUNTIME_ENTRY_VADDR
        || image_plan.base_vaddr as u64 != CONSOLE_NETWORK_RUNTIME_LOAD_BASE_VADDR
        || expected_limit as u64 != CONSOLE_NETWORK_RUNTIME_LOAD_LIMIT_VADDR
        || image_plan.page_count != usize::from(CONSOLE_NETWORK_RUNTIME_LOAD_PAGES)
        || image_plan.page_count != usize::from(object_plan.image_pages)
    {
        return Err(HalError::Unsupported("console-network-image-identity"));
    }

    let tcb = slots[TCB_SLOT_INDEX];
    let child_cnode = slots[CNODE_SLOT_INDEX];
    let vspace = slots[VSPACE_SLOT_INDEX];
    let scheduling_context = slots[SC_SLOT_INDEX];
    let root_to_child_notification = slots[ROOT_TO_CHILD_NOTIFICATION_INDEX];
    let child_to_root_notification = slots[CHILD_TO_ROOT_NOTIFICATION_INDEX];
    retype(anchor, tcb, sel4_sys::seL4_TCBObject as seL4_Word, 0, hal)?;
    retype(
        anchor,
        child_cnode,
        sel4_sys::seL4_CapTableObject as seL4_Word,
        seL4_Word::from(CHILD_CNODE_RADIX_BITS),
        hal,
    )?;
    hal.env
        .create_revoke_anchor_vspace_root(anchor, vspace)
        .map_err(map_vspace_error)?;
    retype(
        anchor,
        scheduling_context,
        sel4_sys::seL4_SchedContextObject as seL4_Word,
        seL4_Word::from(contract.scheduling_context_bits),
        hal,
    )?;
    for notification in [root_to_child_notification, child_to_root_notification] {
        retype(
            anchor,
            notification,
            sel4_sys::seL4_NotificationObject as seL4_Word,
            sel4_sys::seL4_NotificationBits as seL4_Word,
            hal,
        )?;
    }
    for frame_index in 0..FRAME_COUNT {
        retype(
            anchor,
            slots[FRAME_SLOT_START + frame_index],
            sel4_sys::seL4_ARM_SmallPageObject as seL4_Word,
            sel4::PAGE_BITS as seL4_Word,
            hal,
        )?;
    }
    let mut objects_line = heapless::String::<224>::new();
    let _ = core::fmt::write(
        &mut objects_line,
        format_args!(
            "CONSOLE_NETWORK_SHELL phase=objects-ready generation={} tcb=0x{:04x} cnode=0x{:04x} vspace=0x{:04x} sc=0x{:04x} frames={}",
            generation, tcb, child_cnode, vspace, scheduling_context, FRAME_COUNT,
        ),
    );
    crate::bootstrap::log::force_uart_line(objects_line.as_str());

    map_image(hal, anchor, vspace, tracker, &slots, image_plan)?;
    map_zeroed_range(
        hal,
        anchor,
        vspace,
        tracker,
        &slots,
        STACK_FRAME_START,
        usize::from(contract.stack_pages),
        usize::try_from(contract.stack_vaddr)
            .map_err(|_| HalError::Unsupported("console-network-stack-vaddr"))?,
        sel4_sys::seL4_CapRights_ReadWrite,
    )?;
    let ipc_vaddr = usize::try_from(contract.ipc_buffer_vaddr)
        .map_err(|_| HalError::Unsupported("console-network-ipc-vaddr"))?;
    map_zeroed_range(
        hal,
        anchor,
        vspace,
        tracker,
        &slots,
        IPC_FRAME_INDEX,
        1,
        ipc_vaddr,
        sel4_sys::seL4_CapRights_ReadWrite,
    )?;
    map_empty_init_frame(hal, anchor, vspace, tracker, &slots, contract)?;
    let shared_frames =
        map_shared_frames(hal, anchor, vspace, tracker, &slots, contract, generation)?;
    let (direct_virtio_layout, direct_device_cap) =
        map_direct_virtio(hal, anchor, vspace, tracker, &slots)?;
    let (direct_genet_layout, direct_genet_root_ptrs) = map_direct_genet(
        hal,
        anchor,
        vspace,
        tracker,
        &slots,
        generation,
        direct_genet,
    )?;
    hal.env
        .seal_revoke_anchor_translation_reserve(anchor, tracker)
        .map_err(map_vspace_error)?;
    if tracker.mapped_table_count() > TRANSLATION_SLOT_COUNT || tracker.remaining_slots() != 0 {
        return Err(HalError::Unsupported("console-network-page-table-budget"));
    }
    let mut mappings_line = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut mappings_line,
        format_args!(
            "CONSOLE_NETWORK_SHELL phase=mappings-ready generation={} tcb=0x{:04x} tables={} frames={} init_rights=read-only",
            generation,
            tcb,
            tracker.mapped_table_count(),
            FRAME_COUNT,
        ),
    );
    crate::bootstrap::log::force_uart_line(mappings_line.as_str());

    let (root_wake_caps, standard_fault_cap, timeout_fault_cap) = install_caps_and_mcs(
        hal,
        anchor,
        &slots,
        child_cnode,
        tcb,
        vspace,
        scheduling_context,
        root_to_child_notification,
        child_to_root_notification,
        contract,
        ipc_vaddr,
        generation,
        direct_genet_layout.is_some(),
    )?;
    let mut mcs_line = heapless::String::<256>::new();
    let _ = core::fmt::write(
        &mut mcs_line,
        format_args!(
            "CONSOLE_NETWORK_SHELL phase=mcs-ready generation={} tcb=0x{:04x} sc=0x{:04x} standard_fault=0x{:04x} timeout_fault=0x{:04x} registry=registered registers=unset state=suspended",
            generation,
            tcb,
            scheduling_context,
            standard_fault_cap,
            timeout_fault_cap,
        ),
    );
    crate::bootstrap::log::force_uart_line(mcs_line.as_str());

    #[cfg(feature = "net-backend-virtio")]
    let (direct_irq_handler_cap, direct_irq_notification_cap) = (
        slots[DIRECT_IRQ_HANDLER_SLOT_INDEX],
        slots[DIRECT_IRQ_NOTIFICATION_SLOT_INDEX],
    );
    #[cfg(not(feature = "net-backend-virtio"))]
    let (direct_irq_handler_cap, direct_irq_notification_cap) =
        (sel4_sys::seL4_CapNull, sel4_sys::seL4_CapNull);
    let direct_genet_frame_count = if direct_genet_layout.is_some() {
        DIRECT_GENET_FRAME_COPY_SLOT_COUNT as u8
    } else {
        0
    };
    Ok(ConsoleNetworkRuntime {
        boundary,
        anchor,
        slots,
        tracker: tracker.clone(),
        tcb,
        scheduling_context,
        child_to_root_notification,
        root_wake_caps,
        publication_ack_owed: false,
        standard_fault_cap,
        timeout_fault_cap,
        shared_frames,
        direct_virtio_layout,
        direct_genet_layout,
        direct_genet_root_ptrs,
        direct_genet_armed: false,
        direct_genet_child_unmapped: 0,
        direct_genet_caps_deleted: 0,
        direct_device_cap,
        direct_device_child_unmapped: direct_device_cap == sel4_sys::seL4_CapNull,
        direct_device_root_mapping: None,
        direct_device_deleted: direct_device_cap == sel4_sys::seL4_CapNull,
        direct_irq_handler_cap,
        direct_irq_notification_cap,
        direct_dma_child_unmapped: 0,
        direct_dma_root_mapping: None,
        entry: image_plan.entry,
        stack_top,
        init_vaddr,
        descriptor_finalized: false,
        activated: false,
        containment_started: false,
        contained: false,
        containment: ConsoleNetworkContainmentCursor::with_direct_frame_inventories(
            DIRECT_DMA_FRAME_COUNT as u8,
            direct_genet_frame_count,
        ),
        direct_genet_yield_accounting: DirectGenetYieldAccounting::default(),
    })
}

fn retype(
    anchor: seL4_CPtr,
    destination: seL4_CPtr,
    object_type: seL4_Word,
    object_bits: seL4_Word,
    hal: &KernelHal<'_>,
) -> Result<(), HalError> {
    hal.env
        .retype_from_revoke_anchor(anchor, object_type, object_bits, destination)
        .map_err(HalError::Sel4)
}

fn map_image(
    hal: &mut KernelHal<'_>,
    anchor: seL4_CPtr,
    vspace: seL4_CPtr,
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    plan: super::RuntimeElfLoadPlan,
) -> Result<(), HalError> {
    for page_index in 0..plan.page_count {
        let frame_cap = slots[FRAME_SLOT_START + IMAGE_FRAME_START + page_index];
        let mut frame = hal
            .env
            .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
            .map_err(HalError::Sel4)?;
        let fill = fill_runtime_elf_page(
            CONSOLE_NETWORK_RUNTIME_IMAGE,
            plan,
            page_index,
            frame.as_mut_slice(),
        )?;
        super::cache::cache_clean(
            sel4_sys::seL4_CapInitThreadVSpace,
            frame.ptr().as_ptr() as usize,
            SHARED_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
        let vaddr = plan
            .base_vaddr
            .checked_add(page_index << sel4::PAGE_BITS)
            .ok_or(HalError::Unsupported("console-network-image-vaddr"))?;
        let mapping = runtime_elf_page_mapping(fill)?;
        hal.env
            .map_page_cap_into_revoke_anchor_vspace(
                anchor,
                frame_cap,
                vspace,
                vaddr,
                mapping.rights,
                mapping.attributes,
                tracker,
            )
            .map_err(map_vspace_error)?;
        if fill.executable {
            super::cache::cache_unify_instruction(vspace, vaddr, SHARED_PAGE_BYTES)
                .map_err(|error| HalError::Sel4(error.code()))?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn map_zeroed_range(
    hal: &mut KernelHal<'_>,
    anchor: seL4_CPtr,
    vspace: seL4_CPtr,
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    frame_start: usize,
    frame_count: usize,
    vaddr_start: usize,
    rights: sel4_sys::seL4_CapRights,
) -> Result<(), HalError> {
    for index in 0..frame_count {
        let frame_cap = slots[FRAME_SLOT_START + frame_start + index];
        let mut frame = hal
            .env
            .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
            .map_err(HalError::Sel4)?;
        frame.as_mut_slice().fill(0);
        super::cache::cache_clean(
            sel4_sys::seL4_CapInitThreadVSpace,
            frame.ptr().as_ptr() as usize,
            SHARED_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
        let vaddr = vaddr_start
            .checked_add(index << sel4::PAGE_BITS)
            .ok_or(HalError::Unsupported("console-network-zero-vaddr"))?;
        hal.env
            .map_page_cap_into_revoke_anchor_vspace(
                anchor,
                frame_cap,
                vspace,
                vaddr,
                rights,
                runtime_cacheable_xn_attributes(),
                tracker,
            )
            .map_err(map_vspace_error)?;
    }
    Ok(())
}

fn map_empty_init_frame(
    hal: &mut KernelHal<'_>,
    anchor: seL4_CPtr,
    vspace: seL4_CPtr,
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    contract: ConsoleNetworkContract,
) -> Result<(), HalError> {
    let frame_cap = slots[FRAME_SLOT_START + INIT_FRAME_INDEX];
    let mut frame = hal
        .env
        .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
        .map_err(HalError::Sel4)?;
    frame.as_mut_slice().fill(0);
    super::cache::cache_clean(
        sel4_sys::seL4_CapInitThreadVSpace,
        frame.ptr().as_ptr() as usize,
        SHARED_PAGE_BYTES,
    )
    .map_err(|error| HalError::Sel4(error.code()))?;
    hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
    hal.env
        .map_page_cap_into_revoke_anchor_vspace(
            anchor,
            frame_cap,
            vspace,
            usize::try_from(contract.init_vaddr)
                .map_err(|_| HalError::Unsupported("console-network-init-vaddr"))?,
            sel4_sys::seL4_CapRights::new(0, 0, 1, 0),
            runtime_cacheable_xn_attributes(),
            tracker,
        )
        .map_err(map_vspace_error)
}

fn map_shared_frames(
    hal: &mut KernelHal<'_>,
    anchor: seL4_CPtr,
    vspace: seL4_CPtr,
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    contract: ConsoleNetworkContract,
    generation: u64,
) -> Result<Vec<RamFrame, SHARED_FRAME_COUNT>, HalError> {
    let vaddrs = [
        contract.packet_rx_vaddr,
        contract.packet_tx_vaddr,
        contract.command_vaddr,
        contract.event_vaddr,
    ];
    let root_cnode = hal.env.init_cnode_cap();
    let root_depth = sel4::word_bits() as u8;
    let mut frames = Vec::new();
    for (index, vaddr) in vaddrs.into_iter().enumerate() {
        let child_rights = if matches!(index, 0 | 2) {
            sel4_sys::seL4_CapRights::new(0, 0, 1, 0)
        } else {
            sel4_sys::seL4_CapRights_ReadWrite
        };
        let frame_cap = slots[FRAME_SLOT_START + SHARED_FRAME_START + index];
        let mut frame = hal
            .env
            .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
            .map_err(HalError::Sel4)?;
        // A level read can precede the child's first publication. Give every
        // page its exact empty identity while the child is still suspended;
        // direct GENET never writes the legacy packet-egress page at all.
        match index {
            0 => PacketPage::initialize_into(
                frame.as_mut_slice(),
                PacketDirection::Ingress,
                generation,
            ),
            1 => PacketPage::initialize_into(
                frame.as_mut_slice(),
                PacketDirection::Egress,
                generation,
            ),
            _ => ExchangePage::initialize_into(frame.as_mut_slice(), generation),
        }
        .map_err(|_| HalError::Unsupported("console-network-shared-page-init"))?;
        super::cache::cache_clean(
            sel4_sys::seL4_CapInitThreadVSpace,
            frame.ptr().as_ptr() as usize,
            SHARED_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        let child_mapping_cap = slots[SHARED_COPY_SLOT_START + index];
        let error = sel4::cnode_copy_depth(
            root_cnode,
            child_mapping_cap,
            root_depth,
            root_cnode,
            frame_cap,
            root_depth,
            child_rights,
        );
        if error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(error));
        }
        hal.env
            .map_page_cap_into_revoke_anchor_vspace(
                anchor,
                child_mapping_cap,
                vspace,
                usize::try_from(vaddr)
                    .map_err(|_| HalError::Unsupported("console-network-shared-vaddr"))?,
                child_rights,
                runtime_cacheable_xn_attributes(),
                tracker,
            )
            .map_err(map_vspace_error)?;
        frames
            .push(frame)
            .map_err(|_| HalError::Unsupported("console-network-shared-frame-count"))?;
    }
    Ok(frames)
}

#[cfg(feature = "net-backend-genet-direct")]
fn map_direct_genet(
    hal: &mut KernelHal<'_>,
    anchor: seL4_CPtr,
    vspace: seL4_CPtr,
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    generation: u64,
    admitted: bool,
) -> Result<
    (
        Option<DirectGenetLayout>,
        [usize; DIRECT_GENET_SHARED_PAGE_COUNT],
    ),
    HalError,
> {
    if !admitted {
        return Ok((None, [0; DIRECT_GENET_SHARED_PAGE_COUNT]));
    }
    let resources = super::driver_task::driver_task_direct_genet_shared_pages().ok_or(
        HalError::Unsupported("console-network-direct-genet-shared-pages"),
    )?;
    let root_cnode = hal.env.init_cnode_cap();
    let root_depth = sel4::word_bits() as u8;
    let rights = sel4_sys::seL4_CapRights_ReadWrite;
    let mut rx_vaddrs = [0u64; DIRECT_GENET_RX_SLOT_COUNT];
    let mut tx_vaddrs = [0u64; DIRECT_GENET_TX_SLOT_COUNT];
    for index in 0..DIRECT_GENET_SHARED_PAGE_COUNT {
        let destination = slots[DIRECT_GENET_FRAME_COPY_SLOT_START + index];
        let source = seL4_CPtr::try_from(resources.caps[index])
            .map_err(|_| HalError::Unsupported("console-network-direct-genet-frame-cap"))?;
        let copy_error = sel4::cnode_copy_depth(
            root_cnode,
            destination,
            root_depth,
            root_cnode,
            source,
            root_depth,
            rights,
        );
        if copy_error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(copy_error));
        }
        let vaddr = DIRECT_GENET_CONTROL_VADDR
            .checked_add(index * SHARED_PAGE_BYTES)
            .ok_or(HalError::Unsupported("console-network-direct-genet-vaddr"))?;
        hal.env
            .map_external_page_cap_into_revoke_anchor_vspace(
                anchor,
                destination,
                vspace,
                vaddr,
                rights,
                runtime_cacheable_xn_attributes(),
                tracker,
            )
            .map_err(map_vspace_error)?;
        if index > 0 && index <= DIRECT_GENET_RX_SLOT_COUNT {
            rx_vaddrs[index - 1] = vaddr as u64;
        } else if index > DIRECT_GENET_RX_SLOT_COUNT {
            tx_vaddrs[index - 1 - DIRECT_GENET_RX_SLOT_COUNT] = vaddr as u64;
        }
    }
    let layout = DirectGenetLayout {
        magic: DIRECT_GENET_LAYOUT_MAGIC,
        version: DIRECT_GENET_LAYOUT_VERSION,
        layout_bytes: DIRECT_GENET_LAYOUT_BYTES as u16,
        flags: DIRECT_GENET_LAYOUT_FLAGS,
        shared_page_bytes: SHARED_PAGE_BYTES as u16,
        rx_slot_count: DIRECT_GENET_RX_SLOT_COUNT as u8,
        tx_slot_count: DIRECT_GENET_TX_SLOT_COUNT as u8,
        generation,
        peer_wake_notification_slot: DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT,
        reserved0: 0,
        control_vaddr: DIRECT_GENET_CONTROL_VADDR as u64,
        rx_vaddrs,
        tx_vaddrs,
        seal: 0,
    }
    .sealed();
    layout
        .validate_for(generation)
        .map_err(|_| HalError::Unsupported("console-network-direct-genet-layout"))?;
    Ok((Some(layout), resources.root_ptrs))
}

#[cfg(not(feature = "net-backend-genet-direct"))]
fn map_direct_genet(
    _hal: &mut KernelHal<'_>,
    _anchor: seL4_CPtr,
    _vspace: seL4_CPtr,
    _tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    _slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    _generation: u64,
    _admitted: bool,
) -> Result<
    (
        Option<DirectGenetLayout>,
        [usize; DIRECT_GENET_SHARED_PAGE_COUNT],
    ),
    HalError,
> {
    Ok((None, [0; DIRECT_GENET_SHARED_PAGE_COUNT]))
}

#[cfg(feature = "net-backend-virtio")]
fn map_direct_virtio(
    hal: &mut KernelHal<'_>,
    anchor: seL4_CPtr,
    vspace: seL4_CPtr,
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
) -> Result<(Option<DirectVirtioLayout>, seL4_CPtr), HalError> {
    let mut queue_paddrs = [0u64; DIRECT_VIRTIO_QUEUE_COUNT];
    let mut rx_paddrs = [0u64; DIRECT_VIRTIO_BUFFER_COUNT];
    let mut tx_paddrs = [0u64; DIRECT_VIRTIO_BUFFER_COUNT];
    let rights = sel4_sys::seL4_CapRights_ReadWrite;
    for index in 0..DIRECT_DMA_FRAME_COUNT {
        let frame_cap = slots[FRAME_SLOT_START + DIRECT_FRAME_START + index];
        let mut root_frame = hal
            .env
            .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
            .map_err(HalError::Sel4)?;
        root_frame.as_mut_slice().fill(0);
        super::cache::cache_clean(
            sel4_sys::seL4_CapInitThreadVSpace,
            root_frame.ptr().as_ptr() as usize,
            DIRECT_VIRTIO_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        let paddr = u64::try_from(root_frame.paddr())
            .map_err(|_| HalError::Unsupported("console-network-direct-paddr"))?;
        hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
        let (vaddr, destination) = if index < DIRECT_VIRTIO_QUEUE_COUNT {
            (
                DIRECT_VIRTIO_QUEUE_VADDR + index * DIRECT_VIRTIO_PAGE_BYTES,
                &mut queue_paddrs[index],
            )
        } else if index < DIRECT_VIRTIO_QUEUE_COUNT + DIRECT_VIRTIO_BUFFER_COUNT {
            let buffer = index - DIRECT_VIRTIO_QUEUE_COUNT;
            (
                DIRECT_VIRTIO_RX_VADDR + buffer * DIRECT_VIRTIO_PAGE_BYTES,
                &mut rx_paddrs[buffer],
            )
        } else {
            let buffer = index - DIRECT_VIRTIO_QUEUE_COUNT - DIRECT_VIRTIO_BUFFER_COUNT;
            (
                DIRECT_VIRTIO_TX_VADDR + buffer * DIRECT_VIRTIO_PAGE_BYTES,
                &mut tx_paddrs[buffer],
            )
        };
        *destination = paddr;
        hal.env
            .map_page_cap_into_revoke_anchor_vspace(
                anchor,
                frame_cap,
                vspace,
                vaddr,
                rights,
                direct_virtio_dma_attributes(),
                tracker,
            )
            .map_err(map_vspace_error)?;
    }

    let direct_device_cap = hal
        .env
        .map_exclusive_device_page_into_revoke_anchor_vspace(
            anchor,
            DIRECT_VIRTIO_MMIO_PADDR,
            slots[DIRECT_MMIO_SLOT_INDEX],
            vspace,
            DIRECT_VIRTIO_MMIO_VADDR,
            rights,
            direct_virtio_mmio_attributes(),
            tracker,
        )
        .map_err(map_vspace_error)?;
    let layout = DirectVirtioLayout {
        magic: DIRECT_VIRTIO_LAYOUT_MAGIC,
        version: DIRECT_VIRTIO_LAYOUT_VERSION,
        layout_bytes: DIRECT_VIRTIO_LAYOUT_BYTES as u16,
        flags: 0,
        queue_size: DIRECT_VIRTIO_QUEUE_SIZE as u16,
        buffer_count: DIRECT_VIRTIO_BUFFER_COUNT as u16,
        mmio_vaddr: DIRECT_VIRTIO_MMIO_VADDR as u64,
        mmio_paddr: DIRECT_VIRTIO_MMIO_PADDR as u64,
        queue_vaddrs: [
            DIRECT_VIRTIO_QUEUE_VADDR as u64,
            (DIRECT_VIRTIO_QUEUE_VADDR + DIRECT_VIRTIO_PAGE_BYTES) as u64,
        ],
        queue_paddrs,
        rx_vaddr: DIRECT_VIRTIO_RX_VADDR as u64,
        tx_vaddr: DIRECT_VIRTIO_TX_VADDR as u64,
        rx_paddrs,
        tx_paddrs,
        seal: 0,
    }
    .sealed();
    layout
        .validate()
        .map_err(|_| HalError::Unsupported("console-network-direct-layout"))?;
    Ok((Some(layout), direct_device_cap))
}

#[cfg(not(feature = "net-backend-virtio"))]
fn map_direct_virtio(
    _hal: &mut KernelHal<'_>,
    _anchor: seL4_CPtr,
    _vspace: seL4_CPtr,
    _tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    _slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
) -> Result<(Option<DirectVirtioLayout>, seL4_CPtr), HalError> {
    Ok((None, sel4_sys::seL4_CapNull))
}

#[cfg(feature = "net-backend-virtio")]
fn install_direct_virtio_irq(
    hal: &mut KernelHal<'_>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    child_cnode: seL4_CPtr,
    root_to_child_notification: seL4_CPtr,
) -> Result<(), HalError> {
    let root_cnode = hal.env.init_cnode_cap();
    let root_depth = sel4::word_bits() as u8;
    let handler = slots[DIRECT_IRQ_HANDLER_SLOT_INDEX];
    let badged_notification = slots[DIRECT_IRQ_NOTIFICATION_SLOT_INDEX];

    #[cfg(all(target_arch = "aarch64", target_os = "none"))]
    let get_error = sel4::irq_control_get_trigger_handler(
        DIRECT_VIRTIO_IRQ,
        1,
        root_cnode,
        handler,
        root_depth,
    );
    #[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
    let get_error =
        sel4::irq_control_get_level_handler(DIRECT_VIRTIO_IRQ, root_cnode, handler, root_depth);
    if get_error != sel4_sys::seL4_NoError {
        return Err(HalError::Sel4(get_error));
    }

    let result = (|| {
        mint(
            root_cnode,
            badged_notification,
            root_depth,
            root_cnode,
            root_to_child_notification,
            root_depth,
            sel4_sys::seL4_CapRights::new(0, 0, 0, 1),
            seL4_Word::from(WAKE_DIRECT_VIRTIO_IRQ),
        )?;
        let bind_error = sel4::irq_handler_set_notification(handler, badged_notification);
        if bind_error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(bind_error));
        }
        let copy_error = sel4::cnode_copy_depth(
            child_cnode,
            seL4_CPtr::from(DIRECT_VIRTIO_IRQ_HANDLER_SLOT),
            CHILD_CNODE_RADIX_BITS,
            root_cnode,
            handler,
            root_depth,
            sel4_sys::seL4_CapRights_All,
        );
        if copy_error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(copy_error));
        }
        let ack_error = sel4::irq_handler_ack(handler);
        if ack_error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(ack_error));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = sel4::cnode_delete_bounded(
            child_cnode,
            seL4_CPtr::from(DIRECT_VIRTIO_IRQ_HANDLER_SLOT),
            CHILD_CNODE_RADIX_BITS,
        );
        let _ = sel4::irq_handler_clear(handler);
        let _ = sel4::cnode_delete_bounded(root_cnode, badged_notification, root_depth);
        let _ = sel4::cnode_delete_bounded(root_cnode, handler, root_depth);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(feature = "net-backend-virtio"))]
fn install_direct_virtio_irq(
    _hal: &mut KernelHal<'_>,
    _slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    _child_cnode: seL4_CPtr,
    _root_to_child_notification: seL4_CPtr,
) -> Result<(), HalError> {
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn install_caps_and_mcs(
    hal: &mut KernelHal<'_>,
    _anchor: seL4_CPtr,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    child_cnode: seL4_CPtr,
    tcb: seL4_CPtr,
    vspace: seL4_CPtr,
    scheduling_context: seL4_CPtr,
    root_to_child_notification: seL4_CPtr,
    child_to_root_notification: seL4_CPtr,
    contract: ConsoleNetworkContract,
    ipc_vaddr: usize,
    generation: u64,
    direct_genet: bool,
) -> Result<([seL4_CPtr; 5], seL4_CPtr, seL4_CPtr), HalError> {
    let root_cnode = hal.env.init_cnode_cap();
    let root_depth = sel4::word_bits() as u8;
    let child_depth = CHILD_CNODE_RADIX_BITS;
    mint(
        child_cnode,
        seL4_CPtr::from(contract.child_wake_slot),
        child_depth,
        root_cnode,
        root_to_child_notification,
        root_depth,
        sel4_sys::seL4_CapRights::new(0, 0, 1, 0),
        0,
    )?;
    mint(
        child_cnode,
        seL4_CPtr::from(contract.packet_tx_wake_slot),
        child_depth,
        root_cnode,
        child_to_root_notification,
        root_depth,
        sel4_sys::seL4_CapRights::new(0, 0, 0, 1),
        seL4_Word::from(WAKE_PACKET_TX_READY),
    )?;
    mint(
        child_cnode,
        seL4_CPtr::from(contract.supervisor_wake_slot),
        child_depth,
        root_cnode,
        child_to_root_notification,
        root_depth,
        sel4_sys::seL4_CapRights::new(0, 0, 0, 1),
        seL4_Word::from(WAKE_EVENT_READY),
    )?;
    let root_control_wake_notification =
        hal.root_control_wake_notification_origin()
            .ok_or(HalError::Unsupported(
                "console-network-root-control-wake-origin-missing",
            ))?;
    mint(
        child_cnode,
        seL4_CPtr::from(ROOT_CONTROL_WAKE_NOTIFICATION_SLOT),
        child_depth,
        root_cnode,
        root_control_wake_notification,
        root_depth,
        sel4_sys::seL4_CapRights::new(0, 0, 0, 1),
        seL4_Word::from(ROOT_CONTROL_WAKE_NOTIFICATION_BADGE),
    )?;

    let wake_badges = [
        WAKE_PACKET_RX,
        WAKE_CONTROL,
        WAKE_SHUTDOWN,
        WAKE_REVOKE,
        WAKE_PUBLICATION_ACK,
    ];
    let mut root_wake_caps = [sel4_sys::seL4_CapNull; 5];
    for (index, badge) in wake_badges.into_iter().enumerate() {
        let cap = slots[ROOT_WAKE_SLOT_START + index];
        mint(
            root_cnode,
            cap,
            root_depth,
            root_cnode,
            root_to_child_notification,
            root_depth,
            sel4_sys::seL4_CapRights::new(0, 0, 0, 1),
            seL4_Word::from(badge),
        )?;
        root_wake_caps[index] = cap;
    }
    install_direct_virtio_irq(hal, slots, child_cnode, root_to_child_notification)?;
    let direct_genet_peer_caps = hal.install_direct_genet_peer_notifications(
        child_cnode,
        root_to_child_notification,
        direct_genet,
    )?;

    let fault_origin = super::critical_tcb::target_fault_endpoint_origin().ok_or(
        HalError::Unsupported("console-network-critical-fault-endpoint"),
    )?;
    let (standard_badge, timeout_badge) =
        super::critical_tcb::temporal_fault_badges(SERVICE_TASK_ID)
            .ok_or(HalError::Unsupported("console-network-fault-badges"))?;
    if standard_badge != contract.standard_fault_badge || timeout_badge != contract.timeout_badge {
        return Err(HalError::Unsupported("console-network-fault-badge-drift"));
    }
    let standard_fault_cap = slots[STANDARD_FAULT_SLOT_INDEX];
    let timeout_fault_cap = slots[TIMEOUT_FAULT_SLOT_INDEX];
    let fault_rights = sel4_sys::seL4_CapRights::new(1, 0, 0, 1);
    mint(
        root_cnode,
        standard_fault_cap,
        root_depth,
        root_cnode,
        fault_origin,
        root_depth,
        fault_rights,
        seL4_Word::try_from(standard_badge)
            .map_err(|_| HalError::Unsupported("console-network-standard-badge"))?,
    )?;
    mint(
        root_cnode,
        timeout_fault_cap,
        root_depth,
        root_cnode,
        fault_origin,
        root_depth,
        fault_rights,
        seL4_Word::try_from(timeout_badge)
            .map_err(|_| HalError::Unsupported("console-network-timeout-badge"))?,
    )?;

    let sched_control = hal
        .env
        .sched_control_for_core(contract.core)
        .map_err(HalError::Sel4)?;
    let extra_refills = contract
        .max_refills
        .checked_sub(2)
        .ok_or(HalError::Unsupported("console-network-refills"))?;
    sel4::configure_sched_context(
        sched_control,
        scheduling_context,
        u64::from(contract.budget_us),
        u64::from(contract.period_us),
        seL4_Word::from(extra_refills),
        seL4_Word::try_from(timeout_badge)
            .map_err(|_| HalError::Unsupported("console-network-timeout-badge"))?,
        0,
    )
    .map_err(HalError::Sel4)?;

    let guard_bits = sel4::word_bits().saturating_sub(seL4_Word::from(child_depth));
    sel4::set_tcb_space(
        tcb,
        standard_fault_cap,
        child_cnode,
        sel4::cap_data_guard(0, guard_bits),
        vspace,
        0,
    )
    .map_err(HalError::Sel4)?;
    let ipc_frame = slots[FRAME_SLOT_START + IPC_FRAME_INDEX];
    hal.env
        .bind_child_ipc_buffer(tcb, ipc_frame, ipc_vaddr)
        .map_err(HalError::Sel4)?;
    sel4::set_tcb_sched_params_mcs(
        tcb,
        sel4_sys::seL4_CapInitThreadTCB,
        contract.mcp,
        contract.priority,
        scheduling_context,
        standard_fault_cap,
    )
    .map_err(HalError::Sel4)?;
    if requires_timeout_endpoint(contract.timeout_policy) {
        sel4::set_tcb_timeout_endpoint(tcb, timeout_fault_cap).map_err(HalError::Sel4)?;
    }
    let task_index = crate::generated::temporal_tasks()
        .iter()
        .position(|task| task.id == SERVICE_TASK_ID)
        .and_then(|index| u16::try_from(index).ok())
        .ok_or(HalError::Unsupported("console-network-fault-registry-slot"))?;
    let generation = u32::try_from(generation)
        .map_err(|_| HalError::Unsupported("console-network-generation-bound"))?;
    super::critical_tcb::register_target_fault_source(
        SERVICE_TASK_ID,
        tcb,
        GenerationIdentity {
            slot: task_index,
            lease_epoch: 1,
            supervisor_generation: generation,
            cap_generation: generation,
        },
    )
    .map_err(|_| HalError::Unsupported("console-network-fault-register"))?;
    if direct_genet {
        hal.commit_direct_genet_peer_notifications(direct_genet_peer_caps)?;
    }
    Ok((root_wake_caps, standard_fault_cap, timeout_fault_cap))
}

#[allow(clippy::too_many_arguments)]
fn mint(
    destination_root: seL4_CPtr,
    destination: seL4_CPtr,
    destination_depth: u8,
    source_root: seL4_CPtr,
    source: seL4_CPtr,
    source_depth: u8,
    rights: sel4_sys::seL4_CapRights,
    badge: seL4_Word,
) -> Result<(), HalError> {
    let error = sel4::cnode_mint_depth(
        destination_root,
        destination,
        destination_depth,
        source_root,
        source,
        source_depth,
        rights,
        badge,
    );
    if error == sel4_sys::seL4_NoError {
        Ok(())
    } else {
        Err(HalError::Sel4(error))
    }
}

fn map_vspace_error(error: sel4::RevokeAnchorVSpaceError) -> HalError {
    match error {
        sel4::RevokeAnchorVSpaceError::Sel4(error) => HalError::Sel4(error),
        sel4::RevokeAnchorVSpaceError::InvalidDestinationSlots => {
            HalError::Unsupported("console-network-vspace-slots")
        }
        sel4::RevokeAnchorVSpaceError::TranslationObjectBound => {
            HalError::Unsupported("console-network-vspace-bound")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(align(4096))]
    struct AlignedControlPage([u8; SHARED_PAGE_BYTES]);

    #[test]
    fn fixed_slot_plan_accounts_every_generated_object() {
        assert_eq!(FRAME_SLOT_START, 6);
        assert_eq!(TRANSLATION_SLOT_START, FRAME_SLOT_START + FRAME_COUNT);
        assert_eq!(SHARED_COPY_SLOT_START, TRANSLATION_SLOT_START + 8);
        assert_eq!(ROOT_WAKE_SLOT_START, SHARED_COPY_SLOT_START + 4);
        assert_eq!(STANDARD_FAULT_SLOT_INDEX, ROOT_WAKE_SLOT_START + 5);
        assert_eq!(TIMEOUT_FAULT_SLOT_INDEX, STANDARD_FAULT_SLOT_INDEX + 1);
        assert_eq!(
            DIRECT_GENET_FRAME_COPY_SLOT_START,
            TIMEOUT_FAULT_SLOT_INDEX + 1
        );
        assert_eq!(
            DIRECT_MMIO_SLOT_INDEX,
            DIRECT_GENET_FRAME_COPY_SLOT_START + DIRECT_GENET_FRAME_COPY_SLOT_COUNT
        );
        assert_eq!(
            DIRECT_MMIO_SLOT_INDEX + DIRECT_DEVICE_SLOT_COUNT + DIRECT_IRQ_SLOT_COUNT,
            ROOT_SLOT_COUNT
        );
        if DIRECT_IRQ_SLOT_COUNT != 0 {
            assert_eq!(
                DIRECT_IRQ_NOTIFICATION_SLOT_INDEX,
                DIRECT_MMIO_SLOT_INDEX + 1
            );
            assert_eq!(
                DIRECT_IRQ_HANDLER_SLOT_INDEX,
                DIRECT_IRQ_NOTIFICATION_SLOT_INDEX + 1
            );
        }
    }

    #[test]
    fn notification_badges_are_exact_and_directional() {
        assert_eq!(
            WAKE_PACKET_RX | WAKE_CONTROL | WAKE_SHUTDOWN | WAKE_REVOKE,
            15
        );
        assert_eq!(WAKE_PACKET_TX_READY | WAKE_EVENT_READY, CHILD_WAKE_MASK);
        assert_eq!(WAKE_PUBLICATION_ACK, 64);
        assert_eq!(WAKE_DIRECT_VIRTIO_IRQ, 128);
        assert_eq!(ROOT_CONTROL_WAKE_NOTIFICATION_SLOT, 6);
        assert_eq!(ROOT_CONTROL_WAKE_NOTIFICATION_BADGE, 1);
        assert!(ROOT_CONTROL_WAKE_NOTIFICATION_SLOT < (1 << CHILD_CNODE_RADIX_BITS));
    }

    #[test]
    fn same_core_yield_prep_is_exact_to_durable_authenticated_genet_work() {
        assert!(direct_genet_predrain_required(true, true));
        assert!(
            !direct_genet_predrain_required(false, true),
            "mediated WiFi stays signal-only in the dual-mode Pi image",
        );
        assert!(
            !direct_genet_predrain_required(true, false),
            "a refused YieldTo cannot reset child accounting",
        );
        assert!(direct_genet_yield_after_predrain_admitted(true, true));
        assert!(
            !direct_genet_yield_after_predrain_admitted(true, false),
            "a failed SC pre-drain must suppress YieldTo while retaining the durable signal",
        );
        assert!(!direct_genet_yield_after_predrain_admitted(false, true));
        assert!(!direct_genet_yield_after_predrain_admitted(false, false));
        assert_eq!(root_wake_badge(ROOT_CONTROL_WAKE_INDEX), Some(WAKE_CONTROL));
        assert_eq!(
            root_wake_badge(ROOT_PUBLICATION_ACK_WAKE_INDEX),
            Some(WAKE_PUBLICATION_ACK),
        );
        assert_eq!(root_wake_badge(5), None);
        assert_eq!(
            DurableChildPublication::AuthenticatedControl.committed_for_badge(WAKE_CONTROL),
            Ok(true),
        );
        assert_eq!(
            DurableChildPublication::AuthenticatedPublicationCredit
                .committed_for_badge(WAKE_PUBLICATION_ACK),
            Ok(true),
        );
        assert_eq!(
            DurableChildPublication::None.committed_for_badge(WAKE_CONTROL),
            Ok(false),
        );
        assert_eq!(
            DurableChildPublication::AuthenticatedControl.committed_for_badge(WAKE_PUBLICATION_ACK),
            Err(BoundaryError::HandoffFailed),
        );
    }

    #[test]
    fn direct_genet_runtime_diagnostic_reader_rejects_uncommitted_and_stale_records() {
        let generation = 7;
        let mut diagnostic = DirectGenetRuntimeDiagnostic::empty();
        diagnostic.flags = console_network_abi::DIRECT_GENET_RUNTIME_DIAGNOSTIC_FLAG_ACTIVE;
        diagnostic.generation = generation;
        diagnostic.publication_sequence = 3;
        diagnostic.raw_notification_receipts = 7;
        diagnostic.raw_notification_rejected = 1;
        diagnostic.raw_notification_badge_or = 0x500;
        diagnostic.max_slice = console_network_abi::DirectGenetRuntimeSliceReceipt {
            dpc_turn: 4,
            began_ticks: 10,
            finished_ticks: 11,
            stages: 0x11,
            ..console_network_abi::DirectGenetRuntimeSliceReceipt::empty()
        };
        diagnostic.committed_sequence = 3;
        let mut encoded = [0u8; DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES];
        diagnostic
            .encode(&mut encoded)
            .expect("diagnostic fixture is exact");
        let mut page = AlignedControlPage([0; SHARED_PAGE_BYTES]);
        page.0[DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET
            ..DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET + DIRECT_GENET_RUNTIME_DIAGNOSTIC_BYTES]
            .copy_from_slice(&encoded);
        let root_ptr = page.0.as_ptr() as usize;
        assert_eq!(
            sample_direct_genet_runtime_diagnostic(root_ptr, generation),
            Some(diagnostic),
        );
        assert_eq!(
            sample_direct_genet_runtime_diagnostic(root_ptr, generation + 1),
            None,
        );
        let absent_retirement_byte = DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET + 311;
        page.0[absent_retirement_byte] = 1;
        assert_eq!(
            sample_direct_genet_runtime_diagnostic(root_ptr, generation),
            None
        );
        page.0[absent_retirement_byte] = 0;
        assert_eq!(
            sample_direct_genet_runtime_diagnostic(root_ptr, generation),
            Some(diagnostic)
        );
        let commit_ptr = root_ptr
            + DIRECT_GENET_RUNTIME_DIAGNOSTIC_OFFSET
            + DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET;
        // SAFETY: The test page and diagnostic offset are 64-bit aligned; no
        // other test thread accesses this local page.
        unsafe { &*(commit_ptr as *const AtomicU64) }.store(0, Ordering::Release);
        assert_eq!(
            sample_direct_genet_runtime_diagnostic(root_ptr, generation),
            None,
        );
    }

    #[cfg(feature = "net-backend-virtio")]
    #[test]
    fn direct_virtio_ram_is_cache_coherent_while_mmio_is_uncached() {
        let default = sel4::vm_attributes_raw(sel4_sys::seL4_ARM_Page_Default);
        let xn = sel4::vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever);
        assert_eq!(
            sel4::vm_attributes_raw(direct_virtio_dma_attributes()),
            default | xn
        );
        assert_eq!(sel4::vm_attributes_raw(direct_virtio_mmio_attributes()), xn);
    }

    #[test]
    fn only_natural_postpone_omits_the_timeout_endpoint() {
        use crate::generated::TimeoutPolicy;

        assert!(!requires_timeout_endpoint(TimeoutPolicy::NaturalPostpone));
        for policy in [
            TimeoutPolicy::Terminal,
            TimeoutPolicy::ReplenishOnce,
            TimeoutPolicy::ReturnError,
            TimeoutPolicy::FailStop,
        ] {
            assert!(requires_timeout_endpoint(policy));
        }
    }

    #[test]
    fn suspended_shell_cannot_activate_before_descriptor_finalization() {
        let tracker = RevokeAnchorVSpaceTracker::new([1, 2, 3, 4, 5, 6, 7, 8])
            .expect("distinct translation slots");
        let runtime = ConsoleNetworkRuntime {
            boundary: ConsoleNetworkBoundary::new(1).expect("valid generation"),
            anchor: 0,
            slots: [0; ROOT_SLOT_COUNT],
            tracker,
            tcb: 0,
            scheduling_context: 0,
            child_to_root_notification: 0,
            root_wake_caps: [0; 5],
            publication_ack_owed: false,
            standard_fault_cap: 0,
            timeout_fault_cap: 0,
            shared_frames: Vec::new(),
            direct_virtio_layout: None,
            direct_genet_layout: None,
            direct_genet_root_ptrs: [0; DIRECT_GENET_SHARED_PAGE_COUNT],
            direct_genet_armed: false,
            direct_genet_child_unmapped: 0,
            direct_genet_caps_deleted: 0,
            direct_device_cap: 0,
            direct_device_child_unmapped: true,
            direct_device_root_mapping: None,
            direct_device_deleted: true,
            direct_irq_handler_cap: 0,
            direct_irq_notification_cap: 0,
            direct_dma_child_unmapped: 0,
            direct_dma_root_mapping: None,
            entry: 0,
            stack_top: 0,
            init_vaddr: 0,
            descriptor_finalized: false,
            activated: false,
            containment_started: false,
            contained: false,
            containment: ConsoleNetworkContainmentCursor::new(),
            direct_genet_yield_accounting: DirectGenetYieldAccounting::default(),
        };
        assert!(!runtime.descriptor_finalized());
        let mut runtime = runtime;
        assert!(matches!(
            runtime.activate(),
            Err(HalError::Unsupported(
                "console-network-descriptor-not-finalized"
            ))
        ));
        assert!(!runtime.activated());
    }
}
