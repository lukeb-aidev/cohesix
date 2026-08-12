// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Construct and contain the generated isolated console-network child.
// Author: Lukas Bower

//! HAL-owned construction for `console-network-service/v1`.
//!
//! Every child object and translation table is retyped below one retained
//! compiler-selected revoke anchor. The root keeps only copied packet/control
//! pages, one receive notification, four badged send caps, and the TCB/SC caps
//! needed for supervision. Construction leaves the TCB suspended until the
//! complete target fault registry has been sealed.

use core::sync::atomic::{fence, Ordering};

use console_network_abi::{
    CHILD_WAKE_MASK, RUNTIME_INIT_DESCRIPTOR_BYTES, SHARED_PAGE_BYTES, WAKE_CONTROL,
    WAKE_EVENT_READY, WAKE_PACKET_RX, WAKE_PACKET_TX_READY, WAKE_REVOKE, WAKE_SHUTDOWN,
};
use heapless::Vec;
use sel4_sys::{seL4_CPtr, seL4_Word};

use super::{
    fill_runtime_elf_page, plan_runtime_elf_load, runtime_cacheable_xn_attributes,
    runtime_elf_page_mapping, HalError, KernelHal,
};
use crate::console_network_service::{
    BoundaryError, ConsoleNetworkBoundary, ConsoleNetworkContainmentCursor,
    ConsoleNetworkContainmentProof, ConsoleNetworkContainmentTurn, ConsoleNetworkContainmentUnit,
    ConsoleNetworkContract, ConsoleNetworkEvent, ConsoleNetworkObjectPlan,
    CONSOLE_NETWORK_IMAGE_IDENTITY_BOUND, CONSOLE_NETWORK_RUNTIME_ENTRY_VADDR,
    CONSOLE_NETWORK_RUNTIME_IMAGE, CONSOLE_NETWORK_RUNTIME_LOAD_BASE_VADDR,
    CONSOLE_NETWORK_RUNTIME_LOAD_LIMIT_VADDR, CONSOLE_NETWORK_RUNTIME_LOAD_PAGES, SERVICE_TASK_ID,
};
use crate::critical_tcb::GenerationIdentity;
use crate::sel4::{self, RamFrame, RevokeAnchorVSpaceTracker};

const ROOT_SLOT_COUNT: usize = 121;
const TRANSLATION_SLOT_COUNT: usize = 8;
const FRAME_COUNT: usize = 97;
const IMAGE_FRAME_START: usize = 0;
const STACK_FRAME_START: usize = 59;
const IPC_FRAME_INDEX: usize = 91;
const INIT_FRAME_INDEX: usize = 92;
const SHARED_FRAME_START: usize = 93;
const SHARED_FRAME_COUNT: usize = 4;

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
const STANDARD_FAULT_SLOT_INDEX: usize = ROOT_WAKE_SLOT_START + 4;
const TIMEOUT_FAULT_SLOT_INDEX: usize = STANDARD_FAULT_SLOT_INDEX + 1;

const ROOT_PACKET_RX_WAKE_INDEX: usize = 0;
const ROOT_CONTROL_WAKE_INDEX: usize = 1;
const ROOT_SHUTDOWN_WAKE_INDEX: usize = 2;
const ROOT_REVOKE_WAKE_INDEX: usize = 3;

const CHILD_CNODE_RADIX_BITS: u8 = 4;

const _: () = assert!(TIMEOUT_FAULT_SLOT_INDEX < ROOT_SLOT_COUNT);
const _: () = assert!(STACK_FRAME_START + 32 == IPC_FRAME_INDEX);
const _: () = assert!(SHARED_FRAME_START + SHARED_FRAME_COUNT == FRAME_COUNT);
const _: () = assert!(SHARED_FRAME_COUNT == ConsoleNetworkContainmentCursor::SHARED_FRAME_COUNT);
const _: () = assert!(ConsoleNetworkContainmentCursor::FAULT_CAP_COUNT == 2);

/// One nonblocking child-output turn copied into root-owned values.
pub struct ConsoleNetworkTurn {
    /// Validated service event, when the event-ready bit was observed.
    pub event: Option<ConsoleNetworkEvent>,
    /// Validated Ethernet frame, when the packet-ready bit was observed.
    pub egress: Option<Vec<u8, { console_network_abi::ETHERNET_FRAME_BYTES }>>,
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
    root_wake_caps: [seL4_CPtr; 4],
    standard_fault_cap: seL4_CPtr,
    timeout_fault_cap: seL4_CPtr,
    shared_frames: Vec<RamFrame, SHARED_FRAME_COUNT>,
    activated: bool,
    containment_started: bool,
    contained: bool,
    containment: ConsoleNetworkContainmentCursor,
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
        self.activated && !self.contained && self.boundary.ingress_available()
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
        self.activated = false;
        self.containment_started = true;
        Ok(())
    }

    /// Mark a critical-lane fault before root performs complete containment.
    pub fn record_supervisor_fault(&mut self) {
        self.boundary.record_fault();
    }

    /// Resume the child after the target fault registry is sealed.
    pub fn activate(&mut self) -> Result<(), HalError> {
        if self.activated || self.contained {
            return Err(HalError::Unsupported("console-network-activation-state"));
        }
        sel4::resume_tcb(self.tcb).map_err(HalError::Sel4)?;
        self.activated = true;
        Ok(())
    }

    /// Stage one virtual/admitted NIC packet and signal its exact one-hot wake.
    pub fn stage_ingress(&mut self, packet: &[u8]) -> Result<u64, BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        let sequence = self
            .boundary
            .stage_ingress(packet, self.shared_frames[0].as_mut_slice())?;
        fence(Ordering::Release);
        sel4::signal_unchecked(self.root_wake_caps[ROOT_PACKET_RX_WAKE_INDEX]);
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
        fence(Ordering::Release);
        sel4::signal_unchecked(self.root_wake_caps[ROOT_CONTROL_WAKE_INDEX]);
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
        fence(Ordering::Release);
        sel4::signal_unchecked(self.root_wake_caps[ROOT_CONTROL_WAKE_INDEX]);
        Ok(sequence)
    }

    /// Wake one bounded child turn without publishing a new control record.
    pub fn service_tick(&mut self) -> Result<(), BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        fence(Ordering::Release);
        sel4::signal_unchecked(self.root_wake_caps[ROOT_CONTROL_WAKE_INDEX]);
        Ok(())
    }

    /// Whether the newest response for this connection left the TCP send queue.
    #[must_use]
    pub fn console_output_drained(&self, connection_id: u64) -> bool {
        self.boundary.console_output_drained(connection_id)
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
        fence(Ordering::Release);
        sel4::signal_unchecked(self.root_wake_caps[ROOT_REVOKE_WAKE_INDEX]);
        Ok(())
    }

    /// Poll and validate all coalesced child output bits without blocking root.
    pub fn poll_turn(&mut self) -> Result<ConsoleNetworkTurn, BoundaryError> {
        if !self.activated || self.contained {
            return Err(BoundaryError::InvalidState);
        }
        let mut badge = 0;
        let _ = sel4::poll(self.child_to_root_notification, &mut badge);
        if badge == 0 {
            return Ok(ConsoleNetworkTurn {
                event: None,
                egress: None,
            });
        }
        if badge & !(CHILD_WAKE_MASK as seL4_Word) != 0 {
            self.boundary.record_fault();
            return Err(BoundaryError::InvalidRecord);
        }
        fence(Ordering::Acquire);
        let event = if badge & seL4_Word::from(WAKE_EVENT_READY) != 0 {
            Some(
                self.boundary
                    .accept_event(self.shared_frames[3].as_slice())?,
            )
        } else {
            None
        };
        let egress = if badge & seL4_Word::from(WAKE_PACKET_TX_READY) != 0 {
            Some(
                self.boundary
                    .accept_egress(self.shared_frames[1].as_slice())?,
            )
        } else {
            None
        };
        Ok(ConsoleNetworkTurn { event, egress })
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
                sel4::unbind_sched_context_object(self.scheduling_context, self.tcb)
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
        let contract = ConsoleNetworkContract::from_generated()
            .map_err(|_| HalError::Unsupported("console-network-generated-contract"))?;
        let object_plan = ConsoleNetworkObjectPlan::from_generated()
            .map_err(|_| HalError::Unsupported("console-network-object-plan"))?;
        validate_object_plan(object_plan)?;
        let descriptor = contract
            .runtime_init(generation, mac, ipv4, prefix_len, gateway, auth_token)
            .map_err(|_| HalError::Unsupported("console-network-runtime-init"))?;

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
            descriptor,
            anchor,
            slots,
            &mut tracker,
        );
        if result.is_err() {
            let root_cnode = self.env.init_cnode_cap();
            let root_depth = sel4::word_bits() as u8;
            let _ = sel4::cnode_delete(root_cnode, slots[STANDARD_FAULT_SLOT_INDEX], root_depth);
            let _ = sel4::cnode_delete(root_cnode, slots[TIMEOUT_FAULT_SLOT_INDEX], root_depth);
            let _ = self
                .env
                .revoke_anchor_descendants_and_reset_vspace(anchor, &mut tracker);
        }
        result
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
    descriptor: console_network_abi::RuntimeInitDescriptor,
    anchor: seL4_CPtr,
    slots: [seL4_CPtr; ROOT_SLOT_COUNT],
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
) -> Result<ConsoleNetworkRuntime, HalError> {
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
    map_init_descriptor(hal, anchor, vspace, tracker, &slots, contract, descriptor)?;
    let shared_frames = map_shared_frames(hal, anchor, vspace, tracker, &slots, contract)?;
    hal.env
        .seal_revoke_anchor_translation_reserve(anchor, tracker)
        .map_err(map_vspace_error)?;
    if tracker.mapped_table_count() > TRANSLATION_SLOT_COUNT || tracker.remaining_slots() != 0 {
        return Err(HalError::Unsupported("console-network-page-table-budget"));
    }

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
        image_plan.entry,
        descriptor.generation,
    )?;

    let boundary = ConsoleNetworkBoundary::new(descriptor.generation)
        .map_err(|_| HalError::Unsupported("console-network-boundary"))?;
    Ok(ConsoleNetworkRuntime {
        boundary,
        anchor,
        slots,
        tracker: tracker.clone(),
        tcb,
        scheduling_context,
        child_to_root_notification,
        root_wake_caps,
        standard_fault_cap,
        timeout_fault_cap,
        shared_frames,
        activated: false,
        containment_started: false,
        contained: false,
        containment: ConsoleNetworkContainmentCursor::new(),
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

fn map_init_descriptor(
    hal: &mut KernelHal<'_>,
    anchor: seL4_CPtr,
    vspace: seL4_CPtr,
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    contract: ConsoleNetworkContract,
    descriptor: console_network_abi::RuntimeInitDescriptor,
) -> Result<(), HalError> {
    let frame_cap = slots[FRAME_SLOT_START + INIT_FRAME_INDEX];
    let mut frame = hal
        .env
        .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
        .map_err(HalError::Sel4)?;
    frame.as_mut_slice().fill(0);
    let mut encoded = [0u8; RUNTIME_INIT_DESCRIPTOR_BYTES];
    descriptor
        .encode(&mut encoded)
        .map_err(|_| HalError::Unsupported("console-network-init-encode"))?;
    frame.as_mut_slice()[..encoded.len()].copy_from_slice(&encoded);
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
        frame.as_mut_slice().fill(0);
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
    entry: usize,
    generation: u64,
) -> Result<([seL4_CPtr; 4], seL4_CPtr, seL4_CPtr), HalError> {
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

    let wake_badges = [WAKE_PACKET_RX, WAKE_CONTROL, WAKE_SHUTDOWN, WAKE_REVOKE];
    let mut root_wake_caps = [sel4_sys::seL4_CapNull; 4];
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
    sel4::set_tcb_timeout_endpoint(tcb, timeout_fault_cap).map_err(HalError::Sel4)?;
    let stack_top = usize::try_from(contract.stack_vaddr)
        .ok()
        .and_then(|base| base.checked_add(usize::from(contract.stack_pages) << sel4::PAGE_BITS))
        .ok_or(HalError::Unsupported("console-network-stack-top"))?
        & !0xf;
    sel4::write_tcb_registers(
        tcb,
        entry,
        stack_top,
        seL4_Word::try_from(contract.init_vaddr)
            .map_err(|_| HalError::Unsupported("console-network-init-arg"))?,
        false,
    )
    .map_err(HalError::Sel4)?;

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

    #[test]
    fn fixed_slot_plan_accounts_every_generated_object() {
        assert_eq!(FRAME_SLOT_START, 6);
        assert_eq!(TRANSLATION_SLOT_START, 103);
        assert_eq!(SHARED_COPY_SLOT_START, 111);
        assert_eq!(ROOT_WAKE_SLOT_START, 115);
        assert_eq!(STANDARD_FAULT_SLOT_INDEX, 119);
        assert_eq!(TIMEOUT_FAULT_SLOT_INDEX, 120);
        assert_eq!(TIMEOUT_FAULT_SLOT_INDEX + 1, ROOT_SLOT_COUNT);
    }

    #[test]
    fn notification_badges_are_exact_and_directional() {
        assert_eq!(
            WAKE_PACKET_RX | WAKE_CONTROL | WAKE_SHUTDOWN | WAKE_REVOKE,
            15
        );
        assert_eq!(WAKE_PACKET_TX_READY | WAKE_EVENT_READY, CHILD_WAKE_MASK);
    }
}
