// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce the typed bounded root side of the isolated NineDoor service boundary.
// Author: Lukas Bower

//! Root-side sequencing and validation for the isolated NineDoor parser.
//!
//! This module owns no namespace policy. It makes untrusted path and payload
//! bytes cross the pointer-free namespace-service ABI before root applies
//! policy or authoritative mutation. Host tests retain an explicitly
//! in-process compatibility exchange; an `aarch64-unknown-none` build fails
//! closed unless the supervisor supplies a validated target-child config.

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
use core::sync::atomic::{compiler_fence, Ordering};

#[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
use secure9p_transport::validate_namespace_parts;
#[cfg(test)]
use secure9p_transport::SEL4_RIGHTS_WRITE;
use secure9p_transport::{
    validate_namespace_response, NamespaceOpcode, NamespaceRequestHeader, NamespaceResponseView,
    NamespaceRuntimeInitDescriptor, RequestToken, TransportError, TransportState,
    NAMESPACE_HEADER_BYTES, NAMESPACE_PATH_MAX, NAMESPACE_PAYLOAD_MAX, NAMESPACE_PREPARED_LABEL,
    NAMESPACE_REJECTED_LABEL, NAMESPACE_ROOT_CALL_RIGHTS, NAMESPACE_SHARED_FRAME_BYTES,
    SEL4_RIGHTS_READ, SEL4_RIGHTS_READ_WRITE,
};
#[cfg(all(target_arch = "aarch64", target_os = "none"))]
use secure9p_transport::{NAMESPACE_OPERATION_FRAME_BYTES, NAMESPACE_REQUEST_LABEL};

/// Initial generation used only while constructing the first service child.
/// Every teardown must advance the generation before admitting another child.
pub const INITIAL_SERVICE_GENERATION: u64 = 1;
/// Compiler-selected temporal/fault-registry identity for the isolated child.
pub const SERVICE_TASK_ID: &str = "ninedoor-service";

mod generated_image_identity {
    ::core::include!(::core::concat!(
        ::core::env!("OUT_DIR"),
        "/ninedoor_image_identity.rs"
    ));
}

pub use generated_image_identity::{
    NINEDOOR_IMAGE_IDENTITY_BOUND, NINEDOOR_RUNTIME_BYTES, NINEDOOR_RUNTIME_ENTRY_VADDR,
    NINEDOOR_RUNTIME_IMAGE, NINEDOOR_RUNTIME_LOAD_BASE_VADDR, NINEDOOR_RUNTIME_LOAD_LIMIT_VADDR,
    NINEDOOR_RUNTIME_LOAD_PAGES, NINEDOOR_RUNTIME_SHA256,
};

/// Return the compiler-owned target NineDoor service record.
#[must_use]
pub const fn generated_config() -> crate::generated::NineDoorServiceConfig {
    crate::generated::ninedoor_service_config()
}

/// Validated generated construction contract for one passive target service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NineDoorServiceContract {
    /// Selected image identity in the system archive.
    pub image_id: &'static str,
    /// Selected image path in the system archive.
    pub image_path: &'static str,
    /// Required target entry symbol.
    pub entry_symbol: &'static str,
    /// Child CSpace cardinality.
    pub child_cspace_slots: u16,
    /// Retained init-CNode revoke-anchor slot.
    pub revoke_anchor_slot: u32,
    /// Dedicated child-untyped size.
    pub revoke_anchor_bits: u8,
    /// Exact object budget.
    pub objects: crate::generated::KernelObjectBudget,
    /// Child receive endpoint slot.
    pub endpoint_slot: u32,
    /// Child single-owner MCS Reply slot.
    pub reply_slot: u32,
    /// Root-fault restricted CSpace slot holding the recovery Reply cap.
    pub root_fault_recovery_reply_slot: u32,
    /// Child IPC-buffer virtual address.
    pub ipc_buffer_vaddr: u64,
    /// Child read-only init-descriptor virtual address.
    pub init_vaddr: u64,
    /// Child stack base.
    pub stack_vaddr: u64,
    /// Child stack page count.
    pub stack_pages: u16,
    /// Child read-only request window.
    pub request_vaddr: u64,
    /// Child read-write response window.
    pub response_vaddr: u64,
    /// Size of each two-page shared window.
    pub shared_frame_bytes: u32,
    /// Badge delivered with each root request.
    pub request_badge: u64,
    /// Standard fault badge.
    pub fault_badge: u64,
    /// Timeout fault badge.
    pub timeout_badge: u64,
    /// Passive server core locality.
    pub core: u8,
    /// Passive server priority.
    pub priority: u8,
    /// Passive server MCP.
    pub mcp: u8,
    /// One-shot scheduling-context object bits used to reach the first receive.
    pub bootstrap_scheduling_context_bits: u8,
    /// One-shot bootstrap scheduling budget in microseconds.
    pub bootstrap_budget_us: u32,
    /// One-shot bootstrap scheduling period in microseconds.
    pub bootstrap_period_us: u32,
    /// Total bootstrap scheduling-context replenishment bound.
    pub bootstrap_max_refills: u8,
}

impl NineDoorServiceContract {
    /// Validate generated image, object, cap-rights, and temporal truth as one
    /// indivisible passive-service contract.
    pub fn from_generated() -> Result<Self, TransportError> {
        let config = generated_config();
        if !config.enabled
            || config.abi_version != secure9p_transport::NAMESPACE_SERVICE_ABI_VERSION
            || config.image_id != "nine-door-runtime"
            || config.image_path != "cohesix/bin/nine-door-runtime"
            || config.entry_symbol != "_start"
            || config.child_cspace_slots != 16
            || config.endpoint_slot != secure9p_transport::NAMESPACE_SERVICE_ENDPOINT_SLOT as u32
            || config.reply_slot != secure9p_transport::NAMESPACE_SERVICE_REPLY_SLOT as u32
            || config.root_fault_recovery_reply_slot < 10
            || config.shared_frame_bytes as usize != NAMESPACE_SHARED_FRAME_BYTES
            || config.max_inflight != 1
            || config.request_badge == 0
            || config.root_call_rights != NAMESPACE_ROOT_CALL_RIGHTS
            || config.child_receive_rights != secure9p_transport::NAMESPACE_CHILD_RECEIVE_RIGHTS
            || config.root_request_rights != SEL4_RIGHTS_READ_WRITE
            || config.root_response_rights != SEL4_RIGHTS_READ
            || config.child_request_rights != SEL4_RIGHTS_READ
            || config.child_response_rights != SEL4_RIGHTS_READ_WRITE
            || config.bootstrap_scheduling_context_bits
                < crate::generated::worker_resource_admission_config()
                    .object_bits
                    .sched_context_min
            || config.bootstrap_budget_us == 0
            || config.bootstrap_period_us < config.bootstrap_budget_us
            || config.bootstrap_max_refills < 2
        {
            return Err(TransportError::InvalidAbi);
        }
        let temporal = crate::generated::temporal_tasks()
            .iter()
            .find(|task| task.id == SERVICE_TASK_ID)
            .ok_or(TransportError::InvalidAbi)?;
        if temporal.kind != crate::generated::TemporalTaskKind::Service
            || temporal.execution != crate::generated::TemporalExecution::Passive
            || temporal.core != config.core
            || temporal.priority != config.priority
            || temporal.mcp != config.mcp
            || temporal.timeout_badge != config.timeout_badge
            || temporal.timeout_policy != crate::generated::TimeoutPolicy::ReturnError
            || temporal.allowed_donors != ["root-control"]
            || temporal.reply_objects != 1
            || temporal.max_donation_depth != 1
            || temporal.scheduling_context_slot != 0
            || temporal.scheduling_context_bits != 0
        {
            return Err(TransportError::InvalidAbi);
        }
        Ok(Self {
            image_id: config.image_id,
            image_path: config.image_path,
            entry_symbol: config.entry_symbol,
            child_cspace_slots: config.child_cspace_slots,
            revoke_anchor_slot: config.revoke_anchor_slot,
            revoke_anchor_bits: config.revoke_anchor_bits,
            objects: config.objects,
            endpoint_slot: config.endpoint_slot,
            reply_slot: config.reply_slot,
            root_fault_recovery_reply_slot: config.root_fault_recovery_reply_slot,
            ipc_buffer_vaddr: config.ipc_buffer_vaddr,
            init_vaddr: config.init_vaddr,
            stack_vaddr: config.stack_vaddr,
            stack_pages: config.stack_pages,
            request_vaddr: config.request_vaddr,
            response_vaddr: config.response_vaddr,
            shared_frame_bytes: config.shared_frame_bytes,
            request_badge: config.request_badge,
            fault_badge: config.fault_badge,
            timeout_badge: config.timeout_badge,
            core: config.core,
            priority: config.priority,
            mcp: config.mcp,
            bootstrap_scheduling_context_bits: config.bootstrap_scheduling_context_bits,
            bootstrap_budget_us: config.bootstrap_budget_us,
            bootstrap_period_us: config.bootstrap_period_us,
            bootstrap_max_refills: config.bootstrap_max_refills,
        })
    }

    /// Construct the pointer-free child init descriptor for one generation.
    pub fn runtime_descriptor(
        self,
        generation: u64,
    ) -> Result<NamespaceRuntimeInitDescriptor, TransportError> {
        let descriptor = NamespaceRuntimeInitDescriptor {
            version: secure9p_transport::NAMESPACE_RUNTIME_INIT_VERSION,
            descriptor_bytes: secure9p_transport::NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES as u16,
            endpoint_cap_rights: secure9p_transport::NAMESPACE_CHILD_RECEIVE_RIGHTS,
            request_frame_rights: SEL4_RIGHTS_READ,
            response_frame_rights: SEL4_RIGHTS_READ_WRITE,
            reserved_rights: 0,
            generation,
            request_frame_vaddr: self.request_vaddr,
            response_frame_vaddr: self.response_vaddr,
            frame_bytes: self.shared_frame_bytes,
            request_badge: u32::try_from(self.request_badge)
                .map_err(|_| TransportError::InvalidLimits)?,
            endpoint_cptr: u64::from(self.endpoint_slot),
            reply_cptr: u64::from(self.reply_slot),
            reserved: [0; 2],
        };
        if descriptor.valid() {
            Ok(descriptor)
        } else {
            Err(TransportError::InvalidAbi)
        }
    }
}

/// Exact compiler-derived frame and page-table plan for the constructor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NineDoorServiceObjectPlan {
    /// Exact object inventory.
    pub objects: crate::generated::KernelObjectBudget,
    /// Pages available to the validated W^X ELF loader.
    pub image_pages: u16,
}

impl NineDoorServiceObjectPlan {
    /// Derive the fixed image-page count from the compiler-owned total.
    pub fn from_generated() -> Result<Self, TransportError> {
        let config = generated_config();
        let non_image_pages = u32::from(config.stack_pages)
            .checked_add(6)
            .ok_or(TransportError::InvalidLimits)?;
        let image_pages = config
            .objects
            .frames
            .checked_sub(non_image_pages)
            .and_then(|pages| u16::try_from(pages).ok())
            .filter(|pages| *pages != 0)
            .ok_or(TransportError::InvalidLimits)?;
        Ok(Self {
            objects: config.objects,
            image_pages,
        })
    }
}

/// Complete containment evidence for one target service generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NineDoorContainmentProof {
    /// The service TCB was suspended.
    pub tcb_suspended: bool,
    /// All root-shared mappings were zeroed and unmapped.
    pub mappings_scrubbed: bool,
    /// The root-fault recovery Reply cap was removed.
    pub recovery_reply_revoked: bool,
    /// All descendants of the retained anchor were revoked.
    pub capabilities_revoked: bool,
    /// The transport generation was fenced.
    pub generation_fenced: bool,
}

/// Root-side mapping and call-cap contract for one exact service generation.
///
/// The rights bytes are generated attestations, not substitutes for CSpace or
/// VSpace construction checks. The supervisor must mint the endpoint with
/// Write + GrantReply and map request/response frames read-write/read-only in
/// root. [`Self::matches_runtime_descriptor`] verifies the complementary child
/// descriptor before either TCB is resumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetNamespaceServiceConfig {
    /// Root CSpace slot containing the service call endpoint.
    pub endpoint_cptr: u64,
    /// Request-frame virtual address in root's VSpace.
    pub request_frame_vaddr: usize,
    /// Response-frame virtual address in root's VSpace.
    pub response_frame_vaddr: usize,
    /// Exact two-page mapping size for each shared frame.
    pub frame_bytes: usize,
    /// Supervisor generation shared with the child descriptor.
    pub generation: u64,
    /// Nonzero endpoint badge minted into the root call cap.
    pub request_badge: u32,
    /// Generated rights word for root's endpoint cap.
    pub endpoint_cap_rights: u8,
    /// Generated rights word for root's request-frame mapping.
    pub request_frame_rights: u8,
    /// Generated rights word for root's response-frame mapping.
    pub response_frame_rights: u8,
    /// Reserved layout byte; must be zero.
    pub reserved: u8,
}

impl TargetNamespaceServiceConfig {
    /// Validate root-visible slots, mappings, generation, badge, and exact
    /// least-authority rights.
    #[must_use]
    pub const fn valid(self) -> bool {
        self.endpoint_cptr != 0
            && self.request_frame_vaddr != 0
            && self.response_frame_vaddr != 0
            && self.request_frame_vaddr != self.response_frame_vaddr
            && self.request_frame_vaddr & 4095 == 0
            && self.response_frame_vaddr & 4095 == 0
            && self.frame_bytes == NAMESPACE_SHARED_FRAME_BYTES
            && self.generation != 0
            && self.request_badge != 0
            && self.endpoint_cap_rights == NAMESPACE_ROOT_CALL_RIGHTS
            && self.request_frame_rights == SEL4_RIGHTS_READ_WRITE
            && self.response_frame_rights == SEL4_RIGHTS_READ
            && self.reserved == 0
    }

    /// Validate the complementary child descriptor and the shared immutable
    /// generation, badge, and frame bounds.
    #[must_use]
    pub fn matches_runtime_descriptor(self, runtime: NamespaceRuntimeInitDescriptor) -> bool {
        self.valid()
            && runtime.valid()
            && runtime.generation == self.generation
            && runtime.request_badge == self.request_badge
            && runtime.frame_bytes as usize == self.frame_bytes
    }
}

/// Live root mappings paired with one validated generated target config.
///
/// Keeping the frame handles here prevents the target adapter from turning
/// generated integer addresses into Rust references outside root's seL4
/// mapping authority.
#[cfg(all(target_arch = "aarch64", target_os = "none"))]
pub struct TargetNamespaceServiceResources {
    config: TargetNamespaceServiceConfig,
    request_frames: [crate::sel4::RamFrame; 2],
    response_frames: [crate::sel4::RamFrame; 2],
    response_scratch: [u8; NAMESPACE_OPERATION_FRAME_BYTES],
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
impl core::fmt::Debug for TargetNamespaceServiceResources {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("TargetNamespaceServiceResources")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
impl TargetNamespaceServiceResources {
    /// Bind exact root mappings to generated config after construction. Each
    /// mapped page must appear at the corresponding generated root VSpace
    /// address, and request/response physical frames and caps must be disjoint.
    pub fn new(
        config: TargetNamespaceServiceConfig,
        request_frames: [crate::sel4::RamFrame; 2],
        response_frames: [crate::sel4::RamFrame; 2],
    ) -> Result<Self, TransportError> {
        if !config.valid() {
            return Err(TransportError::InvalidAbi);
        }
        let page_bytes = NAMESPACE_SHARED_FRAME_BYTES / 2;
        let request_addresses_match = request_frames.iter().enumerate().all(|(index, frame)| {
            frame.ptr().as_ptr() as usize
                == config.request_frame_vaddr + index.saturating_mul(page_bytes)
        });
        let response_addresses_match = response_frames.iter().enumerate().all(|(index, frame)| {
            frame.ptr().as_ptr() as usize
                == config.response_frame_vaddr + index.saturating_mul(page_bytes)
        });
        let distinct_request = request_frames[0].cap() != request_frames[1].cap()
            && request_frames[0].paddr() != request_frames[1].paddr();
        let distinct_response = response_frames[0].cap() != response_frames[1].cap()
            && response_frames[0].paddr() != response_frames[1].paddr();
        let disjoint_directions = request_frames.iter().all(|request| {
            response_frames.iter().all(|response| {
                request.cap() != response.cap() && request.paddr() != response.paddr()
            })
        });
        if !request_addresses_match
            || !response_addresses_match
            || !distinct_request
            || !distinct_response
            || !disjoint_directions
        {
            return Err(TransportError::InvalidAbi);
        }
        Ok(Self {
            config,
            request_frames,
            response_frames,
            response_scratch: [0; NAMESPACE_OPERATION_FRAME_BYTES],
        })
    }

    /// Return the immutable generated config paired with these live mappings.
    #[must_use]
    pub const fn config(&self) -> TargetNamespaceServiceConfig {
        self.config
    }

    /// Consume the transport mappings during HAL-owned containment.
    pub(crate) fn into_frames(self) -> ([crate::sel4::RamFrame; 2], [crate::sel4::RamFrame; 2]) {
        (self.request_frames, self.response_frames)
    }
}

/// Borrowed typed operation admitted for root policy only after the configured
/// exchange has validated a matching child response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedNamespaceView<'a> {
    header: NamespaceRequestHeader,
    opcode: NamespaceOpcode,
    path: &'a str,
    payload: &'a str,
}

impl<'a> PreparedNamespaceView<'a> {
    /// Return the exact request header attested by the child response.
    #[must_use]
    pub const fn header(self) -> NamespaceRequestHeader {
        self.header
    }

    /// Return the typed namespace operation.
    #[must_use]
    pub const fn opcode(self) -> NamespaceOpcode {
        self.opcode
    }

    /// Return the bounded prepared path.
    #[must_use]
    pub const fn path(self) -> &'a str {
        self.path
    }

    /// Return the bounded prepared payload.
    #[must_use]
    pub const fn payload(self) -> &'a str {
        self.payload
    }
}

/// Root-owned request sequencer for one exact namespace-service generation.
#[derive(Debug)]
pub struct NamespaceServiceBoundary {
    generation: u64,
    next_sequence: u64,
    state: TransportState,
    outstanding: Option<OutstandingNamespaceRequest>,
    #[cfg(all(target_arch = "aarch64", target_os = "none"))]
    target: Option<TargetNamespaceServiceResources>,
}

/// Exact one-in-flight request identity retained until response or cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutstandingNamespaceRequest {
    token: RequestToken,
    opcode: NamespaceOpcode,
}

impl OutstandingNamespaceRequest {
    /// Return the exact request token.
    #[must_use]
    pub const fn token(self) -> RequestToken {
        self.token
    }

    /// Return the expected response operation.
    #[must_use]
    pub const fn opcode(self) -> NamespaceOpcode {
        self.opcode
    }
}

trait NamespaceServiceExchange {
    /// Complete one synchronous preparation exchange. `Ok(())` means a
    /// prepared response with exact request identity and bytes was observed.
    fn prepare(
        &mut self,
        header: NamespaceRequestHeader,
        path: &str,
        payload: &str,
    ) -> Result<(), TransportError>;
}

#[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
struct InProcessCompatibilityExchange;

#[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
impl NamespaceServiceExchange for InProcessCompatibilityExchange {
    fn prepare(
        &mut self,
        header: NamespaceRequestHeader,
        path: &str,
        payload: &str,
    ) -> Result<(), TransportError> {
        validate_namespace_parts(header, path, payload).map(|_| ())
    }
}

impl NamespaceServiceBoundary {
    /// Construct the first compatibility generation. On the target this has
    /// no endpoint and therefore rejects requests until the supervisor uses
    /// [`Self::new_target`].
    #[must_use]
    pub const fn initial() -> Self {
        Self {
            generation: INITIAL_SERVICE_GENERATION,
            next_sequence: 1,
            state: TransportState::Open,
            outstanding: None,
            #[cfg(all(target_arch = "aarch64", target_os = "none"))]
            target: None,
        }
    }

    /// Construct a compatibility boundary for one exact nonzero generation.
    pub const fn new(generation: u64) -> Result<Self, TransportError> {
        if generation == 0 {
            return Err(TransportError::StaleIdentity);
        }
        Ok(Self {
            generation,
            next_sequence: 1,
            state: TransportState::Open,
            outstanding: None,
            #[cfg(all(target_arch = "aarch64", target_os = "none"))]
            target: None,
        })
    }

    /// Construct the target boundary only from a validated generated service
    /// config. The supervisor must separately validate its matching runtime
    /// descriptor before launching the child.
    #[cfg(all(target_arch = "aarch64", target_os = "none"))]
    pub fn new_target(resources: TargetNamespaceServiceResources) -> Result<Self, TransportError> {
        let config = resources.config();
        if !config.valid() {
            return Err(TransportError::InvalidAbi);
        }
        Ok(Self {
            generation: config.generation,
            next_sequence: 1,
            state: TransportState::Open,
            outstanding: None,
            target: Some(resources),
        })
    }

    /// Prepare and validate one operation before root policy observes it.
    pub fn prepare<'a>(
        &mut self,
        opcode: NamespaceOpcode,
        path: &'a str,
        payload: &'a str,
    ) -> Result<PreparedNamespaceView<'a>, TransportError> {
        #[cfg(all(target_arch = "aarch64", target_os = "none"))]
        {
            let mut exchange = self.target.take().ok_or(TransportError::Closed)?;
            let result = self.prepare_with_exchange(&mut exchange, opcode, path, payload);
            self.target = Some(exchange);
            result
        }
        #[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
        {
            let mut exchange = InProcessCompatibilityExchange;
            self.prepare_with_exchange(&mut exchange, opcode, path, payload)
        }
    }

    fn prepare_with_exchange<'a>(
        &mut self,
        exchange: &mut impl NamespaceServiceExchange,
        opcode: NamespaceOpcode,
        path: &'a str,
        payload: &'a str,
    ) -> Result<PreparedNamespaceView<'a>, TransportError> {
        let (header, outstanding) = self.begin(opcode, path.len(), payload.len())?;
        match exchange.prepare(header, path, payload) {
            Ok(()) => {
                self.finish(outstanding);
                Ok(PreparedNamespaceView {
                    header,
                    opcode,
                    path,
                    payload,
                })
            }
            Err(error) => {
                if matches!(
                    error,
                    TransportError::Revoked
                        | TransportError::StaleIdentity
                        | TransportError::InvalidAbi
                ) {
                    self.revoke();
                } else {
                    self.finish(outstanding);
                }
                Err(error)
            }
        }
    }

    /// Reserve the sole in-flight request and return its pointer-free header.
    pub fn begin(
        &mut self,
        opcode: NamespaceOpcode,
        path_len: usize,
        payload_len: usize,
    ) -> Result<(NamespaceRequestHeader, OutstandingNamespaceRequest), TransportError> {
        match self.state {
            TransportState::Open => {}
            TransportState::Closing | TransportState::Closed => return Err(TransportError::Closed),
            TransportState::Revoked => return Err(TransportError::Revoked),
        }
        if self.outstanding.is_some() {
            return Err(TransportError::QueueFull);
        }
        let token = RequestToken::new(self.next_sequence, self.generation)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(TransportError::StaleIdentity)?;
        let header = NamespaceRequestHeader::new(opcode, token, path_len, payload_len)?;
        let outstanding = OutstandingNamespaceRequest { token, opcode };
        self.outstanding = Some(outstanding);
        Ok((header, outstanding))
    }

    /// Admit one child response only when it matches the exact outstanding
    /// sequence, generation, and operation. A mismatch remains outstanding so
    /// the supervisor can cancel or tear down deterministically.
    pub fn accept_response<'a>(
        &mut self,
        outstanding: OutstandingNamespaceRequest,
        response_frame: &'a [u8],
    ) -> Result<NamespaceResponseView<'a>, TransportError> {
        if self.state == TransportState::Revoked {
            return Err(TransportError::Revoked);
        }
        if self.outstanding != Some(outstanding) {
            return Err(TransportError::UnknownRequest);
        }
        let response =
            validate_namespace_response(response_frame, outstanding.token, outstanding.opcode)?;
        self.finish(outstanding);
        Ok(response)
    }

    /// Cancel the exact outstanding request after timeout or caller withdrawal.
    pub fn cancel(
        &mut self,
        outstanding: OutstandingNamespaceRequest,
    ) -> Result<(), TransportError> {
        if self.state == TransportState::Revoked {
            return Err(TransportError::Revoked);
        }
        if self.outstanding != Some(outstanding) {
            return Err(TransportError::UnknownRequest);
        }
        self.finish(outstanding);
        Ok(())
    }

    /// Stop accepting new requests for this generation.
    pub fn close(&mut self) -> Result<(), TransportError> {
        if self.state == TransportState::Revoked {
            return Err(TransportError::Revoked);
        }
        self.state = if self.outstanding.is_some() {
            TransportState::Closing
        } else {
            TransportState::Closed
        };
        Ok(())
    }

    /// Revoke this generation. It cannot be reopened.
    pub fn revoke(&mut self) {
        self.outstanding = None;
        self.state = TransportState::Revoked;
    }

    /// Fence the target generation and return its root mappings to the HAL for
    /// zeroing and unmapping. A second take observes no live authority.
    #[cfg(all(target_arch = "aarch64", target_os = "none"))]
    pub(crate) fn take_target_resources_for_containment(
        &mut self,
    ) -> Option<TargetNamespaceServiceResources> {
        self.revoke();
        self.target.take()
    }

    /// Return the exact generation used for request identity.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Return the bounded transport lifecycle for supervision.
    #[must_use]
    pub const fn state(&self) -> TransportState {
        self.state
    }

    fn finish(&mut self, outstanding: OutstandingNamespaceRequest) {
        if self.outstanding == Some(outstanding) {
            self.outstanding = None;
        }
        if self.state == TransportState::Closing {
            self.state = TransportState::Closed;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ServiceReplyMeta {
    label: u64,
    length: usize,
    extra_caps: usize,
    caps_unwrapped: usize,
    sequence: u64,
    value: u64,
}

fn validate_service_reply(
    request: NamespaceRequestHeader,
    path: &str,
    payload: &str,
    meta: ServiceReplyMeta,
    response_frame: &[u8],
) -> Result<(), TransportError> {
    if meta.length != 2 || meta.extra_caps != 0 || meta.caps_unwrapped != 0 {
        return Err(TransportError::InvalidAbi);
    }
    if meta.sequence != request.sequence {
        return Err(TransportError::StaleIdentity);
    }
    if meta.label == NAMESPACE_REJECTED_LABEL {
        return match TransportError::from_wire_code(meta.value) {
            Ok(error) => Err(error),
            Err(error) => Err(error),
        };
    }
    if meta.label != NAMESPACE_PREPARED_LABEL {
        return Err(TransportError::InvalidAbi);
    }
    let response_len = usize::try_from(meta.value).map_err(|_| TransportError::InvalidLimits)?;
    if !(NAMESPACE_HEADER_BYTES
        ..=NAMESPACE_HEADER_BYTES + NAMESPACE_PATH_MAX + NAMESPACE_PAYLOAD_MAX)
        .contains(&response_len)
        || response_frame.len() != response_len
    {
        return Err(TransportError::InvalidLimits);
    }
    let response = validate_namespace_response(
        response_frame,
        request.token()?,
        NamespaceOpcode::try_from(request.opcode)?,
    )?;
    if response.path() != path || response.payload() != payload {
        return Err(TransportError::InvalidAbi);
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
impl NamespaceServiceExchange for TargetNamespaceServiceResources {
    fn prepare(
        &mut self,
        header: NamespaceRequestHeader,
        path: &str,
        payload: &str,
    ) -> Result<(), TransportError> {
        if !self.config.valid() || header.generation != self.config.generation {
            return Err(TransportError::InvalidAbi);
        }
        let request_len = NAMESPACE_HEADER_BYTES
            .checked_add(path.len())
            .and_then(|len| len.checked_add(payload.len()))
            .ok_or(TransportError::InvalidLimits)?;
        if request_len > self.config.frame_bytes {
            return Err(TransportError::InvalidLimits);
        }

        for frame in &mut self.request_frames {
            frame.as_mut_slice().fill(0);
        }
        let mut encoded_header = [0u8; NAMESPACE_HEADER_BYTES];
        header.encode(&mut encoded_header)?;
        write_shared_bytes(&mut self.request_frames, 0, &encoded_header)?;
        write_shared_bytes(
            &mut self.request_frames,
            NAMESPACE_HEADER_BYTES,
            path.as_bytes(),
        )?;
        write_shared_bytes(
            &mut self.request_frames,
            NAMESPACE_HEADER_BYTES + path.len(),
            payload.as_bytes(),
        )?;
        compiler_fence(Ordering::Release);

        crate::sel4::set_message_register(0, header.sequence as crate::sel4::seL4_Word);
        crate::sel4::set_message_register(1, request_len as crate::sel4::seL4_Word);
        let request_tag = crate::ipc::seL4_MessageInfo::new(
            NAMESPACE_REQUEST_LABEL as crate::sel4::seL4_Word,
            0,
            0,
            2,
        );
        crate::hal::critical_tcb::arm_target_service_call(SERVICE_TASK_ID, header.sequence)
            .map_err(|_| TransportError::Closed)?;
        let reply_result = crate::ipc::try_call(self.config.endpoint_cptr, request_tag);
        let recovery_result =
            crate::hal::critical_tcb::finish_target_service_call(SERVICE_TASK_ID, header.sequence);
        let reply = reply_result.map_err(|_| TransportError::Closed)?;
        recovery_result.map_err(|_| TransportError::InvalidAbi)?;
        compiler_fence(Ordering::Acquire);
        let meta = ServiceReplyMeta {
            label: reply.label() as u64,
            length: reply.length() as usize,
            extra_caps: reply.extra_caps() as usize,
            caps_unwrapped: reply.caps_unwrapped() as usize,
            sequence: crate::sel4::message_register(0) as u64,
            value: crate::sel4::message_register(1) as u64,
        };

        if meta.label != NAMESPACE_PREPARED_LABEL {
            return validate_service_reply(header, path, payload, meta, &[]);
        }
        let response_len =
            usize::try_from(meta.value).map_err(|_| TransportError::InvalidLimits)?;
        if response_len > self.config.frame_bytes {
            return Err(TransportError::InvalidLimits);
        }
        read_shared_bytes(
            &self.response_frames,
            &mut self.response_scratch[..response_len],
        )?;
        validate_service_reply(
            header,
            path,
            payload,
            meta,
            &self.response_scratch[..response_len],
        )
    }
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
fn write_shared_bytes(
    frames: &mut [crate::sel4::RamFrame; 2],
    mut offset: usize,
    mut bytes: &[u8],
) -> Result<(), TransportError> {
    while !bytes.is_empty() {
        let page_bytes = frames[0].as_slice().len();
        let page = offset / page_bytes;
        let page_offset = offset % page_bytes;
        let frame = frames.get_mut(page).ok_or(TransportError::BufferTooSmall)?;
        let copied = bytes.len().min(page_bytes - page_offset);
        frame.as_mut_slice()[page_offset..page_offset + copied].copy_from_slice(&bytes[..copied]);
        offset = offset
            .checked_add(copied)
            .ok_or(TransportError::InvalidLimits)?;
        bytes = &bytes[copied..];
    }
    Ok(())
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
fn read_shared_bytes(
    frames: &[crate::sel4::RamFrame; 2],
    output: &mut [u8],
) -> Result<(), TransportError> {
    let mut copied = 0usize;
    for frame in frames {
        if copied == output.len() {
            break;
        }
        let source = frame.as_slice();
        let count = source.len().min(output.len() - copied);
        output[copied..copied + count].copy_from_slice(&source[..count]);
        copied += count;
    }
    if copied == output.len() {
        Ok(())
    } else {
        Err(TransportError::BufferTooSmall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secure9p_transport::{
        prepare_namespace_operation, NAMESPACE_CHILD_RECEIVE_RIGHTS,
        NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES, NAMESPACE_RUNTIME_INIT_VERSION,
        NAMESPACE_SERVICE_ENDPOINT_SLOT, NAMESPACE_SERVICE_REPLY_SLOT,
    };

    struct PreparedMockExchange {
        corrupt_sequence: bool,
    }

    impl NamespaceServiceExchange for PreparedMockExchange {
        fn prepare(
            &mut self,
            header: NamespaceRequestHeader,
            path: &str,
            payload: &str,
        ) -> Result<(), TransportError> {
            let mut bytes = [0u8; NAMESPACE_PATH_MAX + NAMESPACE_PAYLOAD_MAX];
            bytes[..path.len()].copy_from_slice(path.as_bytes());
            bytes[path.len()..path.len() + payload.len()].copy_from_slice(payload.as_bytes());
            let prepared = prepare_namespace_operation::<NAMESPACE_PATH_MAX, NAMESPACE_PAYLOAD_MAX>(
                header,
                &bytes[..path.len() + payload.len()],
            )?;
            let mut response =
                [0u8; NAMESPACE_HEADER_BYTES + NAMESPACE_PATH_MAX + NAMESPACE_PAYLOAD_MAX];
            let response_len = prepared.encode(&mut response)?;
            validate_service_reply(
                header,
                path,
                payload,
                ServiceReplyMeta {
                    label: NAMESPACE_PREPARED_LABEL,
                    length: 2,
                    extra_caps: 0,
                    caps_unwrapped: 0,
                    sequence: if self.corrupt_sequence {
                        header.sequence.saturating_add(1)
                    } else {
                        header.sequence
                    },
                    value: response_len as u64,
                },
                &response[..response_len],
            )
        }
    }

    fn runtime_descriptor() -> NamespaceRuntimeInitDescriptor {
        NamespaceRuntimeInitDescriptor {
            version: NAMESPACE_RUNTIME_INIT_VERSION,
            descriptor_bytes: NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES as u16,
            endpoint_cap_rights: NAMESPACE_CHILD_RECEIVE_RIGHTS,
            request_frame_rights: SEL4_RIGHTS_READ,
            response_frame_rights: SEL4_RIGHTS_READ_WRITE,
            reserved_rights: 0,
            generation: 7,
            request_frame_vaddr: 0x1000,
            response_frame_vaddr: 0x3000,
            frame_bytes: NAMESPACE_SHARED_FRAME_BYTES as u32,
            request_badge: 5,
            endpoint_cptr: NAMESPACE_SERVICE_ENDPOINT_SLOT,
            reply_cptr: NAMESPACE_SERVICE_REPLY_SLOT,
            reserved: [0; 2],
        }
    }

    fn target_config() -> TargetNamespaceServiceConfig {
        TargetNamespaceServiceConfig {
            endpoint_cptr: 19,
            request_frame_vaddr: 0x5000,
            response_frame_vaddr: 0x7000,
            frame_bytes: NAMESPACE_SHARED_FRAME_BYTES,
            generation: 7,
            request_badge: 5,
            endpoint_cap_rights: NAMESPACE_ROOT_CALL_RIGHTS,
            request_frame_rights: SEL4_RIGHTS_READ_WRITE,
            response_frame_rights: SEL4_RIGHTS_READ,
            reserved: 0,
        }
    }

    #[test]
    fn target_config_matches_only_complementary_child_rights_and_identity() {
        let config = target_config();
        let runtime = runtime_descriptor();
        assert!(config.valid());
        assert!(config.matches_runtime_descriptor(runtime));
        assert!(!TargetNamespaceServiceConfig {
            endpoint_cap_rights: SEL4_RIGHTS_WRITE,
            ..config
        }
        .valid());
        assert!(
            !config.matches_runtime_descriptor(NamespaceRuntimeInitDescriptor {
                generation: 8,
                ..runtime
            })
        );
    }

    #[test]
    fn stale_generation_and_post_revoke_requests_fail_closed() {
        assert!(matches!(
            NamespaceServiceBoundary::new(0),
            Err(TransportError::StaleIdentity)
        ));
        let mut boundary = NamespaceServiceBoundary::new(4).unwrap();
        let prepared = boundary
            .prepare(NamespaceOpcode::Cat, "/proc/boot", "")
            .unwrap();
        assert_eq!(prepared.header().sequence, 1);
        assert_eq!(prepared.header().generation, 4);
        boundary.revoke();
        assert_eq!(
            boundary.prepare(NamespaceOpcode::Cat, "/proc/boot", ""),
            Err(TransportError::Revoked)
        );
    }

    #[test]
    fn malformed_request_is_consumed_without_mutation_authority() {
        let mut boundary = NamespaceServiceBoundary::new(9).unwrap();
        assert_eq!(
            boundary.prepare(NamespaceOpcode::Cat, "/proc/../queen", ""),
            Err(TransportError::InvalidPath)
        );
        let prepared = boundary
            .prepare(NamespaceOpcode::Cat, "/proc/boot", "")
            .unwrap();
        assert_eq!(prepared.header().sequence, 2);
    }

    #[test]
    fn exact_response_clears_one_inflight_slot_and_stale_response_does_not() {
        let mut boundary = NamespaceServiceBoundary::new(11).unwrap();
        let path = "/proc/boot";
        let (header, outstanding) = boundary.begin(NamespaceOpcode::Cat, path.len(), 0).unwrap();
        assert_eq!(
            boundary.begin(NamespaceOpcode::List, 1, 0),
            Err(TransportError::QueueFull)
        );
        let prepared = prepare_namespace_operation::<256, 4096>(header, path.as_bytes()).unwrap();
        let mut response_frame = [0u8; 512];
        let response_len = prepared.encode(&mut response_frame).unwrap();
        response_frame[24] = response_frame[24].wrapping_add(1);
        assert_eq!(
            boundary.accept_response(outstanding, &response_frame[..response_len]),
            Err(TransportError::StaleIdentity)
        );
        response_frame[24] = response_frame[24].wrapping_sub(1);
        let response = boundary
            .accept_response(outstanding, &response_frame[..response_len])
            .unwrap();
        assert_eq!(response.path(), path);
        assert!(boundary.begin(NamespaceOpcode::List, "/".len(), 0).is_ok());
    }

    #[test]
    fn mocked_call_reply_attests_exact_bytes_and_bad_reply_revokes_generation() {
        let mut boundary = NamespaceServiceBoundary::new(13).unwrap();
        let prepared = boundary
            .prepare_with_exchange(
                &mut PreparedMockExchange {
                    corrupt_sequence: false,
                },
                NamespaceOpcode::Echo,
                "/queen/ctl",
                r#"{"spawn":"heart"}"#,
            )
            .unwrap();
        assert_eq!(prepared.path(), "/queen/ctl");
        assert_eq!(prepared.payload(), r#"{"spawn":"heart"}"#);

        assert_eq!(
            boundary.prepare_with_exchange(
                &mut PreparedMockExchange {
                    corrupt_sequence: true,
                },
                NamespaceOpcode::Cat,
                "/proc/boot",
                "",
            ),
            Err(TransportError::StaleIdentity)
        );
        assert_eq!(
            boundary.prepare(NamespaceOpcode::Cat, "/proc/boot", ""),
            Err(TransportError::Revoked)
        );
    }

    #[test]
    fn bootstrap_probe_does_not_consume_the_repeated_call_receive_cycle() {
        let mut boundary = NamespaceServiceBoundary::new(17).unwrap();
        let mut exchange = PreparedMockExchange {
            corrupt_sequence: false,
        };

        for expected_sequence in 1..=4 {
            let prepared = boundary
                .prepare_with_exchange(&mut exchange, NamespaceOpcode::Log, "", "")
                .unwrap();
            assert_eq!(prepared.header().sequence, expected_sequence);
            assert_eq!(prepared.header().generation, 17);
            assert_eq!(prepared.path(), "");
            assert_eq!(prepared.payload(), "");
        }

        assert_eq!(boundary.state(), TransportState::Open);
        assert!(boundary.outstanding.is_none());
    }

    #[test]
    fn failed_bootstrap_fences_every_followup_call() {
        let mut boundary = NamespaceServiceBoundary::new(19).unwrap();
        boundary
            .prepare(NamespaceOpcode::Log, "", "")
            .expect("bootstrap parser probe");
        boundary.revoke();

        assert_eq!(boundary.state(), TransportState::Revoked);
        assert_eq!(
            boundary.prepare(NamespaceOpcode::Log, "", ""),
            Err(TransportError::Revoked)
        );
    }

    #[test]
    fn target_bootstrap_source_preserves_atomic_activation_order() {
        let bridge_source = include_str!("ninedoor.rs");
        let activation = bridge_source
            .split_once("pub fn activate_target_service")
            .unwrap()
            .1
            .split_once("pub fn contain_target_service_if_faulted")
            .unwrap()
            .0;
        let resume = activation.find("NineDoorServiceRuntime::activate").unwrap();
        let probe = activation
            .find(".prepare(NamespaceOpcode::Log, \"\", \"\")")
            .unwrap();
        let validate = activation.find("if probe_result.is_err()").unwrap();
        let unbind = activation
            .find("NineDoorServiceRuntime::finish_bootstrap")
            .unwrap();
        assert!(resume < probe && probe < validate && validate < unbind);
        let probe_suspend = activation
            .find("NineDoorServiceRuntime::fail_bootstrap")
            .unwrap();
        let probe_revoke = probe_suspend
            + activation[probe_suspend..]
                .find("self.namespace_service.revoke();")
                .unwrap();
        assert!(validate < probe_suspend && probe_suspend < probe_revoke);
        assert_eq!(
            activation
                .matches("self.namespace_service.revoke();")
                .count(),
            3
        );
        let finish_failure = activation
            .find("if let Err(error) = finish_result")
            .unwrap();
        let final_revoke = activation
            .rfind("self.namespace_service.revoke();")
            .unwrap();
        assert!(unbind < finish_failure && finish_failure < final_revoke);
        assert!(!activation.contains("configure_sched_context"));
        assert!(!activation.contains("set_tcb_sched_params_mcs"));
        assert!(!activation.contains("set_tcb_affinity"));

        let hal_source = include_str!("hal/ninedoor_service.rs");
        let install_mcs = hal_source
            .split_once("fn install_caps_and_mcs")
            .unwrap()
            .1
            .split_once("fn scrub_root_mapping")
            .unwrap()
            .0;
        let configure = install_mcs.find("configure_sched_context").unwrap();
        let bind = install_mcs.find("set_tcb_sched_params_mcs").unwrap();
        assert!(configure < bind);
        assert!(!install_mcs.contains("resume_tcb"));
        assert!(!install_mcs.contains("set_tcb_affinity"));
        assert!(hal_source.contains("bootstrap_state: BootstrapState::Suspended"));

        let finish_bootstrap = hal_source
            .split_once("pub fn finish_bootstrap")
            .unwrap()
            .1
            .split_once("pub fn fail_bootstrap")
            .unwrap()
            .0;
        let unbind_sc = finish_bootstrap
            .find("unbind_sched_context_object")
            .unwrap();
        let unbind_failure = finish_bootstrap.find("self.fail_bootstrap()").unwrap();
        let passive = finish_bootstrap.find("BootstrapState::Passive").unwrap();
        assert!(unbind_sc < unbind_failure && unbind_failure < passive);
        let fail_bootstrap = hal_source
            .split_once("pub fn fail_bootstrap")
            .unwrap()
            .1
            .split_once("pub fn contain(")
            .unwrap()
            .0;
        let failed_state = fail_bootstrap.find("BootstrapState::Failed").unwrap();
        let suspend = fail_bootstrap.find("suspend_tcb").unwrap();
        assert!(failed_state < suspend);

        let runtime_source = include_str!("../../nine-door-runtime/src/kernel.rs");
        let initial_receive = runtime_source
            .find("let mut tag = receive(descriptor, &mut badge);")
            .unwrap();
        let receive_loop =
            initial_receive + runtime_source[initial_receive..].find("loop {").unwrap();
        let atomic_reply_receive = runtime_source
            .find("tag = reply_receive(descriptor, &mut badge, reply_label);")
            .unwrap();
        assert!(initial_receive < receive_loop);
        assert!(receive_loop < atomic_reply_receive);
        assert_eq!(runtime_source.matches("seL4_Recv(").count(), 1);
        assert_eq!(runtime_source.matches("seL4_ReplyRecv(").count(), 1);
        assert!(!runtime_source.contains("seL4_MCS_Reply("));
    }

    #[test]
    fn reply_metadata_rejects_caps_unknown_labels_and_typed_denials() {
        let token = RequestToken::new(1, 4).unwrap();
        let header = NamespaceRequestHeader::new(NamespaceOpcode::Cat, token, 10, 0).unwrap();
        let base = ServiceReplyMeta {
            label: NAMESPACE_PREPARED_LABEL,
            length: 2,
            extra_caps: 1,
            caps_unwrapped: 0,
            sequence: 1,
            value: NAMESPACE_HEADER_BYTES as u64,
        };
        assert_eq!(
            validate_service_reply(header, "/proc/boot", "", base, &[]),
            Err(TransportError::InvalidAbi)
        );
        assert_eq!(
            validate_service_reply(
                header,
                "/proc/boot",
                "",
                ServiceReplyMeta {
                    label: 99,
                    extra_caps: 0,
                    ..base
                },
                &[],
            ),
            Err(TransportError::InvalidAbi)
        );
        assert_eq!(
            validate_service_reply(
                header,
                "/proc/boot",
                "",
                ServiceReplyMeta {
                    label: NAMESPACE_REJECTED_LABEL,
                    extra_caps: 0,
                    value: TransportError::InvalidPath.wire_code(),
                    ..base
                },
                &[],
            ),
            Err(TransportError::InvalidPath)
        );
    }
}
