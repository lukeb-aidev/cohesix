// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Construct and contain the generated passive NineDoor parser child.
// Author: Lukas Bower

//! HAL-owned construction for the target `namespace-service/v1` boundary.
//!
//! Every child object and translation table is derived from one retained
//! compiler-selected revoke anchor. A bounded bootstrap SC carries the child
//! only through its first receive; after a validated parser probe, root unbinds
//! that exact SC and the allowlisted root-control caller donates its MCS SC
//! through `Call`. Root-fault owns a distinct retained recovery Reply cap that
//! releases that donor exactly once if the parser faults.

use core::fmt;
use core::sync::atomic::{fence, Ordering};

use heapless::Vec;
use secure9p_transport::{
    NamespaceRuntimeInitDescriptor, NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES,
    NAMESPACE_SHARED_FRAME_BYTES,
};
use sel4_sys::{seL4_CPtr, seL4_Word};

use super::{
    fill_runtime_elf_page, plan_runtime_elf_load, runtime_cacheable_xn_attributes,
    runtime_elf_page_mapping, HalError, KernelHal,
};
use crate::critical_tcb::GenerationIdentity;
use crate::ninedoor_service::{
    NamespaceServiceBoundary, NineDoorContainmentCursor, NineDoorContainmentProof,
    NineDoorContainmentTurn, NineDoorContainmentUnit, NineDoorServiceContract,
    NineDoorServiceObjectPlan, TargetNamespaceServiceConfig, TargetNamespaceServiceResources,
    NINEDOOR_IMAGE_IDENTITY_BOUND, NINEDOOR_RUNTIME_ENTRY_VADDR, NINEDOOR_RUNTIME_IMAGE,
    NINEDOOR_RUNTIME_LOAD_BASE_VADDR, NINEDOOR_RUNTIME_LOAD_LIMIT_VADDR,
    NINEDOOR_RUNTIME_LOAD_PAGES, SERVICE_TASK_ID,
};
use crate::sel4::{self, RamFrame, RevokeAnchorVSpaceTracker};

const ROOT_SLOT_COUNT: usize = 80;
const TRANSLATION_SLOT_COUNT: usize = 8;
const FRAME_COUNT: usize = 49;
const IMAGE_FRAME_START: usize = 0;
const IMAGE_FRAME_COUNT: usize = 35;
const STACK_FRAME_START: usize = IMAGE_FRAME_START + IMAGE_FRAME_COUNT;
const IPC_FRAME_INDEX: usize = STACK_FRAME_START + 8;
const INIT_FRAME_INDEX: usize = IPC_FRAME_INDEX + 1;
const SHARED_FRAME_START: usize = INIT_FRAME_INDEX + 1;
const SHARED_FRAME_COUNT: usize = 4;

const TCB_SLOT_INDEX: usize = 0;
const CNODE_SLOT_INDEX: usize = 1;
const VSPACE_SLOT_INDEX: usize = 2;
const ENDPOINT_SLOT_INDEX: usize = 3;
const REPLY_SLOT_INDEX: usize = 4;
const FRAME_SLOT_START: usize = 5;
const TRANSLATION_SLOT_START: usize = FRAME_SLOT_START + FRAME_COUNT;
const CHILD_SHARED_COPY_SLOT_START: usize = TRANSLATION_SLOT_START + TRANSLATION_SLOT_COUNT;
const ROOT_RESPONSE_COPY_SLOT_START: usize = CHILD_SHARED_COPY_SLOT_START + SHARED_FRAME_COUNT;
const ROOT_CALL_SLOT_INDEX: usize = ROOT_RESPONSE_COPY_SLOT_START + 2;
const STANDARD_FAULT_SLOT_INDEX: usize = ROOT_CALL_SLOT_INDEX + 1;
const TIMEOUT_FAULT_SLOT_INDEX: usize = STANDARD_FAULT_SLOT_INDEX + 1;
const BOOTSTRAP_SC_SLOT_INDEX: usize = TIMEOUT_FAULT_SLOT_INDEX + 1;

const CHILD_CNODE_RADIX_BITS: u8 = 4;

const _: () = assert!(SHARED_FRAME_START + SHARED_FRAME_COUNT == FRAME_COUNT);
const _: () = assert!(BOOTSTRAP_SC_SLOT_INDEX < ROOT_SLOT_COUNT);
const _: () = assert!(NineDoorContainmentCursor::REQUEST_FRAME_COUNT == 2);
const _: () = assert!(NineDoorContainmentCursor::RESPONSE_FRAME_COUNT == 2);
const _: () = assert!(NineDoorContainmentCursor::FAULT_CAP_COUNT == 2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BootstrapState {
    Suspended,
    Running,
    Passive,
    Failed,
}

struct NineDoorContainmentResources {
    request_frames: [RamFrame; NineDoorContainmentCursor::REQUEST_FRAME_COUNT],
    response_frames: [RamFrame; NineDoorContainmentCursor::RESPONSE_FRAME_COUNT],
}

/// Live kernel resources for one exact passive NineDoor generation.
pub struct NineDoorServiceRuntime {
    anchor: seL4_CPtr,
    slots: [seL4_CPtr; ROOT_SLOT_COUNT],
    tracker: RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
    tcb: seL4_CPtr,
    bootstrap_scheduling_context: seL4_CPtr,
    standard_fault_cap: seL4_CPtr,
    timeout_fault_cap: seL4_CPtr,
    generation: u64,
    bootstrap_state: BootstrapState,
    contained: bool,
    containment: NineDoorContainmentCursor,
    containment_resources: Option<NineDoorContainmentResources>,
}

impl fmt::Debug for NineDoorServiceRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NineDoorServiceRuntime")
            .field("anchor", &self.anchor)
            .field("tcb", &self.tcb)
            .field("generation", &self.generation)
            .field("bootstrap_state", &self.bootstrap_state)
            .field("contained", &self.contained)
            .field("containment", &self.containment)
            .finish_non_exhaustive()
    }
}

impl NineDoorServiceRuntime {
    /// Exact supervisor generation represented by this runtime.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Resume the receiver on its bounded bootstrap SC only after root-fault
    /// is independently live.
    pub fn activate(&mut self) -> Result<(), HalError> {
        if self.bootstrap_state != BootstrapState::Suspended
            || self.contained
            || !super::critical_tcb::target_fault_receiver_active()
        {
            return Err(HalError::Unsupported("ninedoor-activation-state"));
        }
        sel4::resume_tcb(self.tcb).map_err(HalError::Sel4)?;
        self.bootstrap_state = BootstrapState::Running;
        Ok(())
    }

    /// Unbind the exact one-shot SC after the parser has replied and atomically
    /// entered its next receive. Only this transition makes the service active.
    pub fn finish_bootstrap(&mut self) -> Result<(), HalError> {
        if self.bootstrap_state != BootstrapState::Running || self.contained {
            return Err(HalError::Unsupported("ninedoor-bootstrap-state"));
        }
        if let Err(error) =
            sel4::unbind_sched_context_object(self.bootstrap_scheduling_context, self.tcb, None)
        {
            let close_result = self.fail_bootstrap();
            return match close_result {
                Ok(()) => Err(HalError::Sel4(error)),
                Err(close_error) => Err(close_error),
            };
        }
        self.bootstrap_state = BootstrapState::Passive;
        Ok(())
    }

    /// Suspend a generation whose bootstrap exchange did not validate. The
    /// state is marked failed before the kernel call so no caller can retry or
    /// admit the child even if suspension itself reports an error.
    pub fn fail_bootstrap(&mut self) -> Result<(), HalError> {
        if self.bootstrap_state != BootstrapState::Running || self.contained {
            return Err(HalError::Unsupported("ninedoor-bootstrap-state"));
        }
        self.bootstrap_state = BootstrapState::Failed;
        sel4::suspend_tcb(self.tcb).map_err(HalError::Sel4)
    }

    /// Whether a consumed fault has transferred the transport into containment.
    #[must_use]
    pub const fn containment_active(&self) -> bool {
        self.containment_resources.is_some()
    }

    /// Fence new Calls and retain all four live mappings for bounded teardown.
    pub fn begin_containment(
        &mut self,
        boundary: &mut NamespaceServiceBoundary,
    ) -> Result<(), HalError> {
        if self.contained || self.containment_resources.is_some() {
            return Err(HalError::Unsupported("ninedoor-containment-state"));
        }
        let resources = boundary
            .take_target_resources_for_containment()
            .ok_or(HalError::Unsupported("ninedoor-target-resources"))?;
        let (request_frames, response_frames) = resources.into_frames();
        self.bootstrap_state = BootstrapState::Failed;
        self.containment_resources = Some(NineDoorContainmentResources {
            request_frames,
            response_frames,
        });
        Ok(())
    }

    /// Advance exactly one successor-committed containment unit.
    #[inline(never)]
    pub fn contain_one_turn(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<NineDoorContainmentTurn, HalError> {
        if self.containment_resources.is_none() {
            return Err(HalError::Unsupported("ninedoor-already-contained"));
        }
        let selected = self.containment.select_next();
        let result = match selected {
            NineDoorContainmentUnit::SuspendTcb => {
                sel4::suspend_tcb_bounded(self.tcb).map_err(HalError::Sel4)
            }
            NineDoorContainmentUnit::ScrubCleanRequestFrame(frame_index) => {
                self.scrub_clean_request_frame(frame_index)
            }
            NineDoorContainmentUnit::UnmapRequestFrame(frame_index) => {
                self.unmap_request_frame(hal, frame_index)
            }
            NineDoorContainmentUnit::UnmapResponseRead(frame_index) => {
                self.unmap_response_read(hal, frame_index)
            }
            NineDoorContainmentUnit::MapResponseWritable(frame_index) => {
                self.map_response_writable(frame_index)
            }
            NineDoorContainmentUnit::ScrubCleanResponseWritable(frame_index) => {
                self.scrub_clean_response_writable(frame_index)
            }
            NineDoorContainmentUnit::UnmapResponseWritable(frame_index) => {
                self.unmap_response_writable(hal, frame_index)
            }
            NineDoorContainmentUnit::RevokeRecoveryReply => {
                super::critical_tcb::revoke_target_service_recovery_reply_bounded(SERVICE_TASK_ID)
                    .map_err(|_| HalError::Unsupported("ninedoor-recovery-reply-revoke"))
            }
            NineDoorContainmentUnit::DeleteFaultCap(cap_index) => {
                self.delete_fault_cap(hal, cap_index)
            }
            NineDoorContainmentUnit::RevokeAnchor => hal
                .env
                .revoke_anchor_descendants_and_reset_vspace(self.anchor, &mut self.tracker)
                .map_err(map_vspace_error),
            NineDoorContainmentUnit::Finalize | NineDoorContainmentUnit::Complete => Ok(()),
        };
        if let Err(error) = result {
            self.containment.restore_selected(selected);
            return Err(error);
        }
        if selected != NineDoorContainmentUnit::Complete {
            return Ok(NineDoorContainmentTurn::InProgress);
        }

        self.contained = true;
        let proof = NineDoorContainmentProof {
            tcb_suspended: true,
            mappings_scrubbed: true,
            recovery_reply_revoked: true,
            capabilities_revoked: true,
            generation_fenced: true,
        };
        Ok(NineDoorContainmentTurn::Complete(proof))
    }

    #[inline(never)]
    fn scrub_clean_request_frame(&mut self, frame_index: usize) -> Result<(), HalError> {
        let frame = self
            .containment_resources
            .as_mut()
            .and_then(|resources| resources.request_frames.get_mut(frame_index))
            .ok_or(HalError::Unsupported(
                "ninedoor-containment-request-frame-index",
            ))?;
        scrub_clean_root_mapping(frame)
    }

    #[inline(never)]
    fn unmap_request_frame(
        &mut self,
        hal: &mut KernelHal<'_>,
        frame_index: usize,
    ) -> Result<(), HalError> {
        let frame = self
            .containment_resources
            .as_ref()
            .and_then(|resources| resources.request_frames.get(frame_index))
            .ok_or(HalError::Unsupported(
                "ninedoor-containment-request-frame-index",
            ))?;
        hal.env.unmap_page_cap(frame.cap()).map_err(HalError::Sel4)
    }

    #[inline(never)]
    fn unmap_response_read(
        &mut self,
        hal: &mut KernelHal<'_>,
        frame_index: usize,
    ) -> Result<(), HalError> {
        let frame = self
            .containment_resources
            .as_mut()
            .and_then(|resources| resources.response_frames.get_mut(frame_index))
            .ok_or(HalError::Unsupported(
                "ninedoor-containment-response-frame-index",
            ))?;
        hal.env.unmap_page_cap(frame.cap()).map_err(HalError::Sel4)
    }

    #[inline(never)]
    fn map_response_writable(&mut self, frame_index: usize) -> Result<(), HalError> {
        let frame = self
            .containment_resources
            .as_ref()
            .and_then(|resources| resources.response_frames.get(frame_index))
            .ok_or(HalError::Unsupported(
                "ninedoor-containment-response-frame-index",
            ))?;
        let root_vaddr = frame.ptr().as_ptr() as usize;
        let source_cap = self.slots[FRAME_SLOT_START + SHARED_FRAME_START + 2 + frame_index];
        sel4::map_page_into_vspace_bounded(
            source_cap,
            sel4_sys::seL4_CapInitThreadVSpace,
            root_vaddr,
            sel4_sys::seL4_CapRights_ReadWrite,
            runtime_cacheable_xn_attributes(),
        )
        .map_err(HalError::Sel4)
    }

    #[inline(never)]
    fn scrub_clean_response_writable(&mut self, frame_index: usize) -> Result<(), HalError> {
        let frame = self
            .containment_resources
            .as_mut()
            .and_then(|resources| resources.response_frames.get_mut(frame_index))
            .ok_or(HalError::Unsupported(
                "ninedoor-containment-response-frame-index",
            ))?;
        scrub_clean_root_mapping(frame)
    }

    #[inline(never)]
    fn unmap_response_writable(
        &mut self,
        hal: &mut KernelHal<'_>,
        frame_index: usize,
    ) -> Result<(), HalError> {
        if frame_index >= NineDoorContainmentCursor::RESPONSE_FRAME_COUNT {
            return Err(HalError::Unsupported(
                "ninedoor-containment-response-frame-index",
            ));
        }
        let source_cap = self.slots[FRAME_SLOT_START + SHARED_FRAME_START + 2 + frame_index];
        hal.env.unmap_page_cap(source_cap).map_err(HalError::Sel4)
    }

    #[inline(never)]
    fn delete_fault_cap(&self, hal: &KernelHal<'_>, cap_index: usize) -> Result<(), HalError> {
        let cap = [self.standard_fault_cap, self.timeout_fault_cap]
            .get(cap_index)
            .copied()
            .ok_or(HalError::Unsupported(
                "ninedoor-containment-fault-cap-index",
            ))?;
        let error =
            sel4::cnode_delete_bounded(hal.env.init_cnode_cap(), cap, sel4::word_bits() as u8);
        if error == sel4_sys::seL4_NoError {
            Ok(())
        } else {
            Err(HalError::Sel4(error))
        }
    }
}

impl<'a> KernelHal<'a> {
    /// Construct the exact passive NineDoor child and register it while its TCB
    /// remains suspended.
    pub fn construct_ninedoor_service_runtime(
        &mut self,
        generation: u64,
    ) -> Result<(NineDoorServiceRuntime, NamespaceServiceBoundary), HalError> {
        let contract = NineDoorServiceContract::from_generated()
            .map_err(|_| HalError::Unsupported("ninedoor-generated-contract"))?;
        let object_plan = NineDoorServiceObjectPlan::from_generated()
            .map_err(|_| HalError::Unsupported("ninedoor-object-plan"))?;
        validate_object_plan(object_plan)?;
        let descriptor = contract
            .runtime_descriptor(generation)
            .map_err(|_| HalError::Unsupported("ninedoor-runtime-init"))?;

        let anchor = self
            .env
            .create_revoke_anchor(
                seL4_CPtr::from(contract.revoke_anchor_slot),
                contract.revoke_anchor_bits,
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
            .map_err(|_| HalError::Unsupported("ninedoor-translation-slots"))?;
        let mut tracker = RevokeAnchorVSpaceTracker::new(translation_slots)
            .map_err(|_| HalError::Unsupported("ninedoor-translation-tracker"))?;
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
            let _ = super::critical_tcb::revoke_target_service_recovery_reply(SERVICE_TASK_ID);
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

fn validate_object_plan(plan: NineDoorServiceObjectPlan) -> Result<(), HalError> {
    if plan.objects.tcbs != 1
        || plan.objects.cnodes != 1
        || plan.objects.vspaces != 1
        || plan.objects.page_tables as usize != TRANSLATION_SLOT_COUNT
        || plan.objects.asids != 1
        || plan.objects.frames as usize != FRAME_COUNT
        || plan.objects.endpoints != 1
        || plan.objects.notifications != 0
        || plan.objects.reply_objects != 1
        || plan.objects.scheduling_contexts != 1
        || plan.objects.cspace_slots as usize != ROOT_SLOT_COUNT
        || usize::from(plan.image_pages) != IMAGE_FRAME_COUNT
    {
        return Err(HalError::Unsupported("ninedoor-object-plan-drift"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn construct_generation(
    hal: &mut KernelHal<'_>,
    contract: NineDoorServiceContract,
    object_plan: NineDoorServiceObjectPlan,
    descriptor: NamespaceRuntimeInitDescriptor,
    anchor: seL4_CPtr,
    slots: [seL4_CPtr; ROOT_SLOT_COUNT],
    tracker: &mut RevokeAnchorVSpaceTracker<TRANSLATION_SLOT_COUNT>,
) -> Result<(NineDoorServiceRuntime, NamespaceServiceBoundary), HalError> {
    if !NINEDOOR_IMAGE_IDENTITY_BOUND {
        return Err(HalError::Unsupported("ninedoor-image-unbound"));
    }
    let image_plan = plan_runtime_elf_load(NINEDOOR_RUNTIME_IMAGE, object_plan.image_pages)?;
    let expected_limit = image_plan
        .base_vaddr
        .checked_add(image_plan.page_count << sel4::PAGE_BITS)
        .ok_or(HalError::Unsupported("ninedoor-image-span"))?;
    if image_plan.entry as u64 != NINEDOOR_RUNTIME_ENTRY_VADDR
        || image_plan.base_vaddr as u64 != NINEDOOR_RUNTIME_LOAD_BASE_VADDR
        || expected_limit as u64 != NINEDOOR_RUNTIME_LOAD_LIMIT_VADDR
        || image_plan.page_count != usize::from(NINEDOOR_RUNTIME_LOAD_PAGES)
        || image_plan.page_count != usize::from(object_plan.image_pages)
    {
        return Err(HalError::Unsupported("ninedoor-image-identity"));
    }

    let tcb = slots[TCB_SLOT_INDEX];
    let child_cnode = slots[CNODE_SLOT_INDEX];
    let vspace = slots[VSPACE_SLOT_INDEX];
    let endpoint = slots[ENDPOINT_SLOT_INDEX];
    let reply = slots[REPLY_SLOT_INDEX];
    let bootstrap_scheduling_context = slots[BOOTSTRAP_SC_SLOT_INDEX];
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
        endpoint,
        sel4_sys::seL4_EndpointObject as seL4_Word,
        sel4_sys::seL4_EndpointBits as seL4_Word,
        hal,
    )?;
    retype(
        anchor,
        reply,
        sel4_sys::seL4_ReplyObject as seL4_Word,
        sel4_sys::SEL4_MCS_REPLY_BITS as seL4_Word,
        hal,
    )?;
    retype(
        anchor,
        bootstrap_scheduling_context,
        sel4_sys::seL4_SchedContextObject as seL4_Word,
        seL4_Word::from(contract.bootstrap_scheduling_context_bits),
        hal,
    )?;
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
            .map_err(|_| HalError::Unsupported("ninedoor-stack-vaddr"))?,
        sel4_sys::seL4_CapRights_ReadWrite,
    )?;
    let ipc_vaddr = usize::try_from(contract.ipc_buffer_vaddr)
        .map_err(|_| HalError::Unsupported("ninedoor-ipc-vaddr"))?;
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
    let (request_frames, response_frames) =
        map_shared_frames(hal, anchor, vspace, tracker, &slots, contract)?;
    hal.env
        .seal_revoke_anchor_translation_reserve(anchor, tracker)
        .map_err(map_vspace_error)?;
    if tracker.mapped_table_count() > TRANSLATION_SLOT_COUNT || tracker.remaining_slots() != 0 {
        return Err(HalError::Unsupported("ninedoor-page-table-budget"));
    }

    let (root_call_cap, standard_fault_cap, timeout_fault_cap) = install_caps_and_mcs(
        hal,
        &slots,
        child_cnode,
        tcb,
        vspace,
        endpoint,
        reply,
        bootstrap_scheduling_context,
        contract,
        ipc_vaddr,
        image_plan.entry,
        descriptor.generation,
    )?;
    let root_config = TargetNamespaceServiceConfig {
        endpoint_cptr: root_call_cap as u64,
        request_frame_vaddr: request_frames[0].ptr().as_ptr() as usize,
        response_frame_vaddr: response_frames[0].ptr().as_ptr() as usize,
        frame_bytes: NAMESPACE_SHARED_FRAME_BYTES,
        generation: descriptor.generation,
        request_badge: descriptor.request_badge,
        endpoint_cap_rights: contract_root_call_rights(),
        request_frame_rights: secure9p_transport::SEL4_RIGHTS_READ_WRITE,
        response_frame_rights: secure9p_transport::SEL4_RIGHTS_READ,
        reserved: 0,
    };
    if !root_config.matches_runtime_descriptor(descriptor) {
        return Err(HalError::Unsupported("ninedoor-root-child-contract"));
    }
    let resources =
        TargetNamespaceServiceResources::new(root_config, request_frames, response_frames)
            .map_err(|_| HalError::Unsupported("ninedoor-root-resources"))?;
    let boundary = NamespaceServiceBoundary::new_target(resources)
        .map_err(|_| HalError::Unsupported("ninedoor-boundary"))?;
    Ok((
        NineDoorServiceRuntime {
            anchor,
            slots,
            tracker: tracker.clone(),
            tcb,
            bootstrap_scheduling_context,
            standard_fault_cap,
            timeout_fault_cap,
            generation: descriptor.generation,
            bootstrap_state: BootstrapState::Suspended,
            contained: false,
            containment: NineDoorContainmentCursor::new(),
            containment_resources: None,
        },
        boundary,
    ))
}

const fn contract_root_call_rights() -> u8 {
    secure9p_transport::NAMESPACE_ROOT_CALL_RIGHTS
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
            NINEDOOR_RUNTIME_IMAGE,
            plan,
            page_index,
            frame.as_mut_slice(),
        )?;
        super::cache::cache_clean(
            sel4_sys::seL4_CapInitThreadVSpace,
            frame.ptr().as_ptr() as usize,
            sel4::IPC_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
        let vaddr = plan
            .base_vaddr
            .checked_add(page_index << sel4::PAGE_BITS)
            .ok_or(HalError::Unsupported("ninedoor-image-vaddr"))?;
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
            super::cache::cache_unify_instruction(vspace, vaddr, sel4::IPC_PAGE_BYTES)
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
            sel4::IPC_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
        let vaddr = vaddr_start
            .checked_add(index << sel4::PAGE_BITS)
            .ok_or(HalError::Unsupported("ninedoor-zero-vaddr"))?;
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
    contract: NineDoorServiceContract,
    descriptor: NamespaceRuntimeInitDescriptor,
) -> Result<(), HalError> {
    let frame_cap = slots[FRAME_SLOT_START + INIT_FRAME_INDEX];
    let mut frame = hal
        .env
        .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
        .map_err(HalError::Sel4)?;
    frame.as_mut_slice().fill(0);
    descriptor
        .encode(&mut frame.as_mut_slice()[..NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES])
        .map_err(|_| HalError::Unsupported("ninedoor-init-encode"))?;
    super::cache::cache_clean(
        sel4_sys::seL4_CapInitThreadVSpace,
        frame.ptr().as_ptr() as usize,
        sel4::IPC_PAGE_BYTES,
    )
    .map_err(|error| HalError::Sel4(error.code()))?;
    hal.env.unmap_page_cap(frame_cap).map_err(HalError::Sel4)?;
    hal.env
        .map_page_cap_into_revoke_anchor_vspace(
            anchor,
            frame_cap,
            vspace,
            usize::try_from(contract.init_vaddr)
                .map_err(|_| HalError::Unsupported("ninedoor-init-vaddr"))?,
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
    contract: NineDoorServiceContract,
) -> Result<([RamFrame; 2], [RamFrame; 2]), HalError> {
    let mut root_sources = Vec::<RamFrame, SHARED_FRAME_COUNT>::new();
    for index in 0..SHARED_FRAME_COUNT {
        let frame_cap = slots[FRAME_SLOT_START + SHARED_FRAME_START + index];
        let mut frame = hal
            .env
            .map_revoke_anchor_frame_in_root(frame_cap, runtime_cacheable_xn_attributes())
            .map_err(HalError::Sel4)?;
        frame.as_mut_slice().fill(0);
        super::cache::cache_clean(
            sel4_sys::seL4_CapInitThreadVSpace,
            frame.ptr().as_ptr() as usize,
            sel4::IPC_PAGE_BYTES,
        )
        .map_err(|error| HalError::Sel4(error.code()))?;
        root_sources
            .push(frame)
            .map_err(|_| HalError::Unsupported("ninedoor-shared-frame-count"))?;
    }

    let root_cnode = hal.env.init_cnode_cap();
    let root_depth = sel4::word_bits() as u8;
    for index in 0..SHARED_FRAME_COUNT {
        let source = slots[FRAME_SLOT_START + SHARED_FRAME_START + index];
        let child_copy = slots[CHILD_SHARED_COPY_SLOT_START + index];
        let child_rights = if index < 2 {
            sel4_sys::seL4_CapRights::new(0, 0, 1, 0)
        } else {
            sel4_sys::seL4_CapRights_ReadWrite
        };
        let error = sel4::cnode_copy_depth(
            root_cnode,
            child_copy,
            root_depth,
            root_cnode,
            source,
            root_depth,
            child_rights,
        );
        if error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(error));
        }
        let child_vaddr = if index < 2 {
            contract.request_vaddr + (index as u64 * sel4::IPC_PAGE_BYTES as u64)
        } else {
            contract.response_vaddr + ((index - 2) as u64 * sel4::IPC_PAGE_BYTES as u64)
        };
        hal.env
            .map_page_cap_into_revoke_anchor_vspace(
                anchor,
                child_copy,
                vspace,
                usize::try_from(child_vaddr)
                    .map_err(|_| HalError::Unsupported("ninedoor-shared-vaddr"))?,
                child_rights,
                runtime_cacheable_xn_attributes(),
                tracker,
            )
            .map_err(map_vspace_error)?;
    }

    let mut sources = root_sources.into_iter();
    let request_frames = [
        sources
            .next()
            .ok_or(HalError::Unsupported("ninedoor-request-frame"))?,
        sources
            .next()
            .ok_or(HalError::Unsupported("ninedoor-request-frame"))?,
    ];
    let response_sources = [
        sources
            .next()
            .ok_or(HalError::Unsupported("ninedoor-response-frame"))?,
        sources
            .next()
            .ok_or(HalError::Unsupported("ninedoor-response-frame"))?,
    ];
    for source in &response_sources {
        hal.env
            .unmap_page_cap(source.cap())
            .map_err(HalError::Sel4)?;
    }
    let mut root_responses = Vec::<RamFrame, 2>::new();
    for (index, source) in response_sources.iter().enumerate() {
        let read_cap = slots[ROOT_RESPONSE_COPY_SLOT_START + index];
        let error = sel4::cnode_copy_depth(
            root_cnode,
            read_cap,
            root_depth,
            root_cnode,
            source.cap(),
            root_depth,
            sel4_sys::seL4_CapRights::new(0, 0, 1, 0),
        );
        if error != sel4_sys::seL4_NoError {
            return Err(HalError::Sel4(error));
        }
        root_responses
            .push(
                hal.env
                    .map_revoke_anchor_frame_in_root_with_rights(
                        read_cap,
                        sel4_sys::seL4_CapRights::new(0, 0, 1, 0),
                        runtime_cacheable_xn_attributes(),
                    )
                    .map_err(HalError::Sel4)?,
            )
            .map_err(|_| HalError::Unsupported("ninedoor-response-frame-count"))?;
    }
    let mut responses = root_responses.into_iter();
    let response_frames = [
        responses
            .next()
            .ok_or(HalError::Unsupported("ninedoor-response-frame"))?,
        responses
            .next()
            .ok_or(HalError::Unsupported("ninedoor-response-frame"))?,
    ];
    Ok((request_frames, response_frames))
}

#[allow(clippy::too_many_arguments)]
fn install_caps_and_mcs(
    hal: &mut KernelHal<'_>,
    slots: &[seL4_CPtr; ROOT_SLOT_COUNT],
    child_cnode: seL4_CPtr,
    tcb: seL4_CPtr,
    vspace: seL4_CPtr,
    endpoint: seL4_CPtr,
    reply: seL4_CPtr,
    bootstrap_scheduling_context: seL4_CPtr,
    contract: NineDoorServiceContract,
    ipc_vaddr: usize,
    entry: usize,
    generation: u64,
) -> Result<(seL4_CPtr, seL4_CPtr, seL4_CPtr), HalError> {
    let root_cnode = hal.env.init_cnode_cap();
    let root_depth = sel4::word_bits() as u8;
    mint(
        child_cnode,
        seL4_CPtr::from(contract.endpoint_slot),
        CHILD_CNODE_RADIX_BITS,
        root_cnode,
        endpoint,
        root_depth,
        sel4_sys::seL4_CapRights::new(0, 0, 1, 0),
        0,
    )?;
    let error = sel4::cnode_copy_depth(
        child_cnode,
        seL4_CPtr::from(contract.reply_slot),
        CHILD_CNODE_RADIX_BITS,
        root_cnode,
        reply,
        root_depth,
        sel4_sys::seL4_CapRights_All,
    );
    if error != sel4_sys::seL4_NoError {
        return Err(HalError::Sel4(error));
    }
    let root_call_cap = slots[ROOT_CALL_SLOT_INDEX];
    mint(
        root_cnode,
        root_call_cap,
        root_depth,
        root_cnode,
        endpoint,
        root_depth,
        sel4_sys::seL4_CapRights::new(1, 0, 0, 1),
        seL4_Word::try_from(contract.request_badge)
            .map_err(|_| HalError::Unsupported("ninedoor-request-badge"))?,
    )?;

    let fault_origin = super::critical_tcb::target_fault_endpoint_origin()
        .ok_or(HalError::Unsupported("ninedoor-critical-fault-endpoint"))?;
    let (standard_badge, timeout_badge) =
        super::critical_tcb::temporal_fault_badges(SERVICE_TASK_ID)
            .ok_or(HalError::Unsupported("ninedoor-fault-badges"))?;
    if standard_badge != contract.fault_badge || timeout_badge != contract.timeout_badge {
        return Err(HalError::Unsupported("ninedoor-fault-badge-drift"));
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
            .map_err(|_| HalError::Unsupported("ninedoor-standard-badge"))?,
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
            .map_err(|_| HalError::Unsupported("ninedoor-timeout-badge"))?,
    )?;

    let sched_control = hal
        .env
        .sched_control_for_core(contract.core)
        .map_err(HalError::Sel4)?;
    let extra_refills = contract
        .bootstrap_max_refills
        .checked_sub(2)
        .ok_or(HalError::Unsupported("ninedoor-bootstrap-refills"))?;
    sel4::configure_sched_context(
        sched_control,
        bootstrap_scheduling_context,
        u64::from(contract.bootstrap_budget_us),
        u64::from(contract.bootstrap_period_us),
        seL4_Word::from(extra_refills),
        seL4_Word::try_from(timeout_badge)
            .map_err(|_| HalError::Unsupported("ninedoor-timeout-badge"))?,
        0,
    )
    .map_err(HalError::Sel4)?;

    let guard_bits = sel4::word_bits().saturating_sub(seL4_Word::from(CHILD_CNODE_RADIX_BITS));
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
        bootstrap_scheduling_context,
        standard_fault_cap,
    )
    .map_err(HalError::Sel4)?;
    sel4::set_tcb_timeout_endpoint(tcb, timeout_fault_cap).map_err(HalError::Sel4)?;
    let stack_top = usize::try_from(contract.stack_vaddr)
        .ok()
        .and_then(|base| base.checked_add(usize::from(contract.stack_pages) << sel4::PAGE_BITS))
        .ok_or(HalError::Unsupported("ninedoor-stack-top"))?
        & !0xf;
    sel4::write_tcb_registers(
        tcb,
        entry,
        stack_top,
        seL4_Word::try_from(contract.init_vaddr)
            .map_err(|_| HalError::Unsupported("ninedoor-init-arg"))?,
        false,
    )
    .map_err(HalError::Sel4)?;

    super::critical_tcb::register_target_service_recovery_reply(SERVICE_TASK_ID, reply)
        .map_err(|_| HalError::Unsupported("ninedoor-recovery-reply-register"))?;
    let task_index = crate::generated::temporal_tasks()
        .iter()
        .position(|task| task.id == SERVICE_TASK_ID)
        .and_then(|index| u16::try_from(index).ok())
        .ok_or(HalError::Unsupported("ninedoor-fault-registry-slot"))?;
    let generation = u32::try_from(generation)
        .map_err(|_| HalError::Unsupported("ninedoor-generation-bound"))?;
    if super::critical_tcb::register_target_fault_source(
        SERVICE_TASK_ID,
        tcb,
        GenerationIdentity {
            slot: task_index,
            lease_epoch: 1,
            supervisor_generation: generation,
            cap_generation: generation,
        },
    )
    .is_err()
    {
        let _ = super::critical_tcb::revoke_target_service_recovery_reply(SERVICE_TASK_ID);
        return Err(HalError::Unsupported("ninedoor-fault-register"));
    }
    Ok((root_call_cap, standard_fault_cap, timeout_fault_cap))
}

fn scrub_clean_root_mapping(frame: &mut RamFrame) -> Result<(), HalError> {
    frame.as_mut_slice().fill(0);
    fence(Ordering::Release);
    super::cache::cache_clean_bounded(
        sel4_sys::seL4_CapInitThreadVSpace,
        frame.ptr().as_ptr() as usize,
        sel4::IPC_PAGE_BYTES,
    )
    .map_err(|error| HalError::Sel4(error.code()))
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
            HalError::Unsupported("ninedoor-vspace-slots")
        }
        sel4::RevokeAnchorVSpaceError::TranslationObjectBound => {
            HalError::Unsupported("ninedoor-vspace-bound")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_slot_plan_accounts_for_every_passive_child_resource() {
        assert_eq!(FRAME_SLOT_START, 5);
        assert_eq!(TRANSLATION_SLOT_START, 54);
        assert_eq!(CHILD_SHARED_COPY_SLOT_START, 62);
        assert_eq!(ROOT_RESPONSE_COPY_SLOT_START, 66);
        assert_eq!(ROOT_CALL_SLOT_INDEX, 68);
        assert_eq!(STANDARD_FAULT_SLOT_INDEX, 69);
        assert_eq!(TIMEOUT_FAULT_SLOT_INDEX, 70);
        assert_eq!(BOOTSTRAP_SC_SLOT_INDEX, 71);
        assert!(BOOTSTRAP_SC_SLOT_INDEX < ROOT_SLOT_COUNT);
    }

    #[test]
    fn generated_frame_arithmetic_has_no_hidden_page_pool() {
        assert_eq!(
            IMAGE_FRAME_COUNT + 8 + 1 + 1 + SHARED_FRAME_COUNT,
            FRAME_COUNT
        );
    }
}
