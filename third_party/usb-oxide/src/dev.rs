// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! USB device abstraction and context structures.

use crate::{
    Dma, Result, UsbError,
    desc::{DeviceDesc, EndpointDesc, SetupPacket, desc_type},
    reg,
    ring::{PhysMem, Ring, Trb, completion, trb_type},
    xhci::XhciCtrl,
};

use alloc::{sync::Arc, vec::Vec};
use core::hint::spin_loop;
use core::ptr;
use core::sync::atomic::{Ordering, compiler_fence};
use spin::Mutex;

const CONTROL_XFER_WAIT_SPINS: usize = 20_000_000;
const ROUTE_STRING_MASK: u32 = 0x000f_ffff;
const ROOT_HUB_PORT_SHIFT: u32 = 16;
const SLOT_DEV_MTT: u32 = 1 << 25;
const TT_PORT_SHIFT: u32 = 8;
const CONTEXT_ALIGN_BYTES: usize = 64;
const CONFIG_DESC_MIN_LEN: usize = 9;
const CONFIG_DESC_MAX_LEN: usize = 4096;
const CONFIG_DESC_HEADER_RETRIES: usize = 3;
const MAX_ENDPOINT_CONTEXTS: usize = 31;
const DEVICE_CONTEXT_ENTRIES: usize = 1 + MAX_ENDPOINT_CONTEXTS;
const INPUT_CONTEXT_ENTRIES: usize = 2 + MAX_ENDPOINT_CONTEXTS;

/// xHCI Slot Context (32 bytes).
///
/// Contains device-specific information used by the xHCI controller
/// to manage USB device communication.
#[repr(C, align(32))]
#[derive(Clone, Copy, Default)]
pub struct SlotContext {
    /// Route String, Speed, Multi-TT, Hub, Context Entries
    pub dw0: u32,
    /// Max Exit Latency, Root Hub Port Number, Number of Ports
    pub dw1: u32,
    /// Interrupter Target, TTT, TT Port Number, TT Hub Slot ID
    pub dw2: u32,
    /// Device Address, Slot State
    pub dw3: u32,
    _0: [u32; 4],
}

impl SlotContext {
    /// Creates a new Slot Context.
    pub fn new(route: u32, speed: u8, context_entries: u8, root_hub_port: u8) -> Self {
        Self {
            dw0: (route & ROUTE_STRING_MASK)
                | ((speed as u32) << 20)
                | ((context_entries as u32) << 27),
            dw1: (root_hub_port as u32) << ROOT_HUB_PORT_SHIFT,
            dw2: 0,
            dw3: 0,
            _0: [0; 4],
        }
    }
}

/// Transaction Translator (TT) context for FS/LS devices behind a HS hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtContext {
    /// Hub slot ID hosting the TT.
    pub hub_slot_id: u8,
    /// Downstream hub port number (USB numbering, starts at 1).
    pub downstream_port: u8,
    /// Whether the parent hub advertises Multi-TT support.
    pub multi_tt: bool,
}

/// xHCI Endpoint Context (32 bytes).
///
/// Defines the characteristics and state of a USB endpoint.
#[repr(C, align(32))]
#[derive(Clone, Copy, Default)]
pub struct EndpointContext {
    /// EP State, Mult, MaxPStreams, LSA, Interval, Max ESIT Payload Hi
    pub dw0: u32,
    /// CErr, EP Type, HID, Max Burst Size, Max Packet Size
    pub dw1: u32,
    /// Transfer Ring Dequeue Pointer Low
    pub tr_dequeue_lo: u32,
    /// Transfer Ring Dequeue Pointer High
    pub tr_dequeue_hi: u32,
    /// Average TRB Length, Max ESIT Payload Lo
    pub dw4: u32,
    _0: [u32; 3],
}

impl EndpointContext {
    /// Creates a new Endpoint Context.
    pub fn new(
        ep_type: u8,
        max_packet_size: u16,
        max_burst: u8,
        interval: u8,
        tr_ptr: u64,
    ) -> Self {
        Self {
            dw0: (interval as u32) << 16,
            dw1: ((3u32) << 1)
                | ((ep_type as u32) << 3)
                | ((max_burst as u32) << 8)
                | ((max_packet_size as u32) << 16),
            tr_dequeue_lo: (tr_ptr as u32) | 1, // DCS = 1
            tr_dequeue_hi: (tr_ptr >> 32) as u32,
            dw4: 8, // Average TRB Length
            _0: [0; 3],
        }
    }
}

/// xHCI Input Context for Address Device / Configure Endpoint commands.
///
/// Used to pass configuration data to the xHCI controller when
/// addressing a device or configuring endpoints.
#[repr(C, align(64))]
#[derive(Default)]
pub struct InputContext {
    /// Input Control Context (drop/add flags)
    pub input_control: [u32; 8],
    /// Slot Context
    pub slot: SlotContext,
    /// Endpoint Contexts (EP0 at index 0, EP1 OUT at 1, EP1 IN at 2, etc.)
    pub endpoints: [EndpointContext; 31],
}

/// xHCI Device Context.
///
/// Output context maintained by the xHCI controller containing
/// the current state of a USB device's slot and endpoints.
#[repr(C, align(64))]
#[derive(Default)]
pub struct DeviceContext {
    /// Slot Context
    pub slot: SlotContext,
    /// Endpoint Contexts
    pub endpoints: [EndpointContext; 31],
}

/// USB Device abstraction.
///
/// Represents an addressed USB device connected to an xHCI controller.
/// Provides methods for control transfers, device enumeration, and
/// endpoint configuration.
pub struct UsbDevice<H: Dma> {
    ctrl: Arc<XhciCtrl<H>>,
    slot_id: u8,
    port: u8,
    root_hub_port: u8,
    route: u32,
    speed: u8,
    device_ctx: PhysMem<H>,
    input_ctx: PhysMem<H>,
    ep0_ring: Mutex<Ring<H>>,
    ep_rings: Mutex<Vec<Option<Ring<H>>>>,
    device_desc: Option<DeviceDesc>,
}

impl<H: Dma> UsbDevice<H> {
    /// Create and address a new USB device on a root hub port.
    pub fn new(ctrl: Arc<XhciCtrl<H>>, port: u8) -> Result<Self> {
        ctrl.reset_port(port)?;
        let speed = ctrl.port_speed(port);
        let root_hub_port = port.saturating_add(1);
        Self::new_with_topology(ctrl, port, 0, root_hub_port, speed, None)
    }

    /// Create and address a new USB device routed behind a hub.
    pub fn new_routed(
        ctrl: Arc<XhciCtrl<H>>,
        route: u32,
        root_hub_port: u8,
        speed: u8,
        tt_context: Option<TtContext>,
    ) -> Result<Self> {
        let port = root_hub_port.checked_sub(1).ok_or(UsbError::InvPort)?;
        Self::new_with_topology(ctrl, port, route, root_hub_port, speed, tt_context)
    }

    #[inline]
    fn ctx_stride(&self) -> usize {
        self.ctrl.context_size_bytes()
    }

    #[inline]
    fn input_drop_flags_ptr(&self) -> *mut u32 {
        self.input_ctx.as_ptr::<u32>()
    }

    #[inline]
    fn input_add_flags_ptr(&self) -> *mut u32 {
        // Input Control Context: dword 1
        self.input_ctx.as_ptr::<u8>().wrapping_add(4).cast::<u32>()
    }

    #[inline]
    fn input_ep_ctx_ptr(&self, ep_ctx_index: usize) -> *mut EndpointContext {
        // EP context index 0 == EP0, 1 == EP1 OUT, 2 == EP1 IN, ...
        let offset = (2 + ep_ctx_index).saturating_mul(self.ctx_stride());
        self.input_ctx
            .as_ptr::<u8>()
            .wrapping_add(offset)
            .cast::<EndpointContext>()
    }

    #[inline]
    fn output_slot_ctx_ptr(&self) -> *const SlotContext {
        self.device_ctx.as_ptr::<SlotContext>()
    }

    #[inline]
    fn output_ep0_ctx_ptr(&self) -> *const EndpointContext {
        self.device_ctx
            .as_ptr::<u8>()
            .wrapping_add(self.ctx_stride())
            .cast::<EndpointContext>()
    }

    fn emit_control_failure_context(
        &self,
        setup: &SetupPacket,
        data_len: usize,
        completion_ptr: u64,
        completion_code: u8,
        stage: u8,
        waited: usize,
    ) {
        // SAFETY: Output slot/endpoint contexts are controller-owned memory
        // allocated by this device and remain mapped for the device lifetime.
        let slot_ctx = unsafe { ptr::read_volatile(self.output_slot_ctx_ptr()) };
        // SAFETY: See slot context safety note above.
        let ep0_ctx = unsafe { ptr::read_volatile(self.output_ep0_ctx_ptr()) };
        let slot_state = ((slot_ctx.dw3 >> 27) & 0x1f) as u64;
        let ep0_state = (ep0_ctx.dw0 & 0x7) as u64;
        let waited_clamped = core::cmp::min(waited, u32::MAX as usize) as u64;
        self.ctrl.emit_diag(
            0x033b,
            completion_ptr,
            ((completion_code as u64) << 56)
                | ((stage as u64) << 48)
                | ((slot_state as u64) << 40)
                | ((ep0_state as u64) << 32)
                | waited_clamped,
            ((setup.request_type as u64) << 56)
                | ((setup.request as u64) << 48)
                | ((setup.value as u64) << 32)
                | (setup.index as u64),
        );
        self.ctrl.emit_diag(
            0x033c,
            ((setup.length as u64) << 48) | (data_len as u64),
            self.ctrl.device_context_entry(self.slot_id),
            ((ep0_ctx.tr_dequeue_hi as u64) << 32) | ep0_ctx.tr_dequeue_lo as u64,
        );
        self.ctrl.emit_diag(
            0x033d,
            ((slot_ctx.dw0 as u64) << 32) | slot_ctx.dw1 as u64,
            ((slot_ctx.dw2 as u64) << 32) | slot_ctx.dw3 as u64,
            ((ep0_ctx.dw0 as u64) << 32) | ep0_ctx.dw1 as u64,
        );
        self.ctrl.emit_diag(
            0x033e,
            ep0_ctx.dw4 as u64,
            self.device_ctx.phys(self.ctrl.host()),
            self.input_ctx.phys(self.ctrl.host()),
        );
    }

    fn recover_ep0_after_failure(
        &self,
        completion_ptr: u64,
        completion_code: u8,
        stage: u8,
    ) {
        let host = self.ctrl.host();
        let (enqueue_idx, producer_cycle, dequeue_ptr) = {
            let ep0_ring = self.ep0_ring.lock();
            let (enqueue_idx, producer_cycle) = ep0_ring.debug_state();
            (
                enqueue_idx,
                producer_cycle,
                ep0_ring.phys(host) + (enqueue_idx * 16) as u64,
            )
        };

        self.ctrl.emit_diag(
            0x033f,
            completion_ptr,
            ((completion_code as u64) << 56)
                | ((stage as u64) << 48)
                | ((self.slot_id as u64) << 40)
                | (1u64 << 32) // EP0 xHCI endpoint ID
                | enqueue_idx as u64,
            ((producer_cycle as u64) << 63) | dequeue_ptr,
        );

        let reset_result = self.ctrl.reset_endpoint(self.slot_id, 1);
        self.ctrl.emit_diag(
            0x0340,
            ((self.slot_id as u64) << 32) | 1u64,
            dequeue_ptr,
            match reset_result {
                Ok(()) => 0,
                Err(UsbError::CmdFail(code)) => (1u64 << 32) | code as u64,
                Err(UsbError::Timeout) => 2u64 << 32,
                Err(_) => 3u64 << 32,
            },
        );

        if reset_result.is_err() {
            return;
        }

        let dequeue_result = self
            .ctrl
            .set_tr_dequeue(self.slot_id, 1, dequeue_ptr, producer_cycle);
        self.ctrl.emit_diag(
            0x0341,
            ((self.slot_id as u64) << 32) | 1u64,
            dequeue_ptr,
            match dequeue_result {
                Ok(()) => 0,
                Err(UsbError::CmdFail(code)) => (1u64 << 32) | code as u64,
                Err(UsbError::Timeout) => 2u64 << 32,
                Err(_) => 3u64 << 32,
            },
        );
    }

    fn new_with_topology(
        ctrl: Arc<XhciCtrl<H>>,
        port: u8,
        route: u32,
        root_hub_port: u8,
        speed: u8,
        tt_context: Option<TtContext>,
    ) -> Result<Self> {
        if root_hub_port == 0 {
            return Err(UsbError::InvPort);
        }
        if tt_context
            .as_ref()
            .is_some_and(|tt| tt.hub_slot_id == 0 || tt.downstream_port == 0)
        {
            return Err(UsbError::InvPort);
        }

        let host = ctrl.host();
        let tt_ctx = tt_context;

        // Enable slot
        let mut slot_id = ctrl.enable_slot()?;
        let enumerated_speed = speed;

        let ctx_stride = ctrl.context_size_bytes();
        if ctx_stride != 32 && ctx_stride != 64 {
            return Err(UsbError::NotSupported);
        }

        // Allocate contexts using controller-reported context stride.
        let device_ctx_bytes = DEVICE_CONTEXT_ENTRIES
            .checked_mul(ctx_stride)
            .ok_or(UsbError::OoRam)?;
        let input_ctx_bytes = INPUT_CONTEXT_ENTRIES
            .checked_mul(ctx_stride)
            .ok_or(UsbError::OoRam)?;

        let device_ctx = PhysMem::alloc(
            host,
            device_ctx_bytes,
            CONTEXT_ALIGN_BYTES,
        )?;
        let input_ctx = PhysMem::alloc(
            host,
            input_ctx_bytes,
            CONTEXT_ALIGN_BYTES,
        )?;

        // Allocate EP0 transfer ring
        let ep0_ring = Ring::new(host, 256)?;

        let input_base = input_ctx.as_ptr::<u8>();
        let build_slot_ctx = |slot_speed: u8| {
            let mut slot_ctx = SlotContext::new(route, slot_speed, 1, root_hub_port);
            if let Some(tt) = tt_ctx {
                if tt.multi_tt {
                    slot_ctx.dw0 |= SLOT_DEV_MTT;
                }
                slot_ctx.dw2 =
                    (tt.hub_slot_id as u32) | ((tt.downstream_port as u32) << TT_PORT_SHIFT);
            }
            slot_ctx
        };
        let slot_ctx_template = build_slot_ctx(enumerated_speed);

        // Set device context in DCBAA
        unsafe {
            core::ptr::write_bytes(device_ctx.as_ptr::<u8>(), 0, device_ctx_bytes);
            core::ptr::write_bytes(input_base, 0, input_ctx_bytes);
        }
        ctrl.set_device_context(slot_id, device_ctx.phys(host));
        compiler_fence(Ordering::Release);
        ctrl.emit_diag(
            0x0320,
            ((slot_id as u64) << 32) | port as u64,
            device_ctx.phys(host),
            ctrl.device_context_entry(slot_id),
        );

        // Build EP0 max-packet candidates. Some controllers/devices are strict
        // about the initial EP0 MPS used during Address Device.
        let build_ep0_mps_candidates = |slot_speed: u8| {
            let mut candidates = [0u16; 3];
            let mut count = 0usize;
            let mut push_candidate = |mps: u16| {
                if !candidates[..count].contains(&mps) && count < 3 {
                    candidates[count] = mps;
                    count += 1;
                }
            };
            match slot_speed {
                reg::SPEED_LOW => push_candidate(8),
                reg::SPEED_FULL => {
                    push_candidate(8);
                    push_candidate(64);
                }
                reg::SPEED_HIGH => push_candidate(64),
                reg::SPEED_SUPER | reg::SPEED_SUPER_PLUS => push_candidate(512),
                _ => {
                    push_candidate(8);
                    push_candidate(64);
                    push_candidate(512);
                }
            }
            (candidates, count)
        };
        let (ep0_mps_candidates, candidate_count) = build_ep0_mps_candidates(enumerated_speed);
        let encode_port_state = |portsc: u32| -> u64 {
            let speed = reg::portsc_speed(portsc) as u64;
            let pls = reg::portsc_pls(portsc) as u64;
            let ped = ((portsc & reg::PORTSC_PED) != 0) as u64;
            let ccs = ((portsc & reg::PORTSC_CCS) != 0) as u64;
            (speed << 56) | (pls << 48) | (ped << 40) | (ccs << 32) | portsc as u64
        };

        const MAX_SLOT_RECYCLES: usize = 1;
        let mut slot_recycles = 0usize;
        'address_attempt: loop {
            let mut last_ctx_state_err: Option<UsbError> = None;
            for (attempt_idx, max_packet) in ep0_mps_candidates[..candidate_count]
                .iter()
                .copied()
                .enumerate()
            {
                unsafe {
                    // Reinitialize both output and input contexts before each
                    // Address Device submission so stale controller state
                    // cannot leak across retries.
                    core::ptr::write_bytes(device_ctx.as_ptr::<u8>(), 0, device_ctx_bytes);
                    core::ptr::write_bytes(input_base, 0, input_ctx_bytes);
                    // Input Control Context:
                    // - Drop flags (dword 0) = 0
                    // - Add flags  (dword 1) = Slot + EP0
                    *(input_base as *mut u32) = 0;
                    *(input_base.add(4) as *mut u32) = 0b11;
                    *(input_base.add(ctx_stride) as *mut SlotContext) = slot_ctx_template;
                    *(input_base.add(ctx_stride * 2) as *mut EndpointContext) =
                        EndpointContext::new(
                            4, // Control Bidirectional
                            max_packet,
                            0,
                            0,
                            ep0_ring.phys(host),
                        );
                }
                compiler_fence(Ordering::Release);

                // xHCI diagnostics: address-attempt and programmed slot/ep0 context.
                let slot_ctx = unsafe { *(input_base.add(ctx_stride) as *const SlotContext) };
                let ep0_ctx =
                    unsafe { *(input_base.add(ctx_stride * 2) as *const EndpointContext) };
                ctrl.emit_diag(
                    0x0310,
                    ((attempt_idx as u64) << 32) | enumerated_speed as u64,
                    ((slot_id as u64) << 32) | max_packet as u64,
                    slot_recycles as u64,
                );
                ctrl.emit_diag(
                    0x0311,
                    ((slot_ctx.dw0 as u64) << 32) | slot_ctx.dw1 as u64,
                    ((slot_ctx.dw2 as u64) << 32) | slot_ctx.dw3 as u64,
                    0,
                );
                ctrl.emit_diag(
                    0x0312,
                    ((ep0_ctx.dw0 as u64) << 32) | ep0_ctx.dw1 as u64,
                    ((ep0_ctx.tr_dequeue_hi as u64) << 32) | ep0_ctx.tr_dequeue_lo as u64,
                    ep0_ctx.dw4 as u64,
                );
                let input_ctrl = unsafe { core::slice::from_raw_parts(input_base as *const u32, 8) };
                let portsc_before_addr = ctrl.port_status(port);
                ctrl.emit_diag(
                    0x0315,
                    input_ctx.phys(host),
                    device_ctx.phys(host),
                    ep0_ring.phys(host),
                );
                ctrl.emit_diag(
                    0x0316,
                    ((input_ctrl[0] as u64) << 32) | input_ctrl[1] as u64,
                    ((input_ctrl[2] as u64) << 32) | input_ctrl[3] as u64,
                    ((input_ctrl[4] as u64) << 32) | input_ctrl[5] as u64,
                );
                ctrl.emit_diag(
                    0x0317,
                    ((input_ctrl[6] as u64) << 32) | input_ctrl[7] as u64,
                    ((portsc_before_addr as u64) << 32) | slot_id as u64,
                    ((route as u64) << 32)
                        | ((root_hub_port as u64) << 16)
                        | ((enumerated_speed as u64) << 8)
                        | port as u64,
                );
                ctrl.emit_diag(
                    0x0322,
                    port as u64,
                    encode_port_state(portsc_before_addr),
                    ((slot_id as u64) << 32) | attempt_idx as u64,
                );
                ctrl.emit_diag(
                    0x0321,
                    input_ctx.phys(host),
                    device_ctx.phys(host),
                    ctrl.device_context_entry(slot_id),
                );

                // Address Device command
                let trb = Trb {
                    param: input_ctx.phys(host),
                    status: 0,
                    control: (trb_type::ADDRESS_DEVICE << 10) | ((slot_id as u32) << 24),
                };
                match ctrl.submit_command(trb) {
                    Ok(_) => break 'address_attempt,
                    Err(UsbError::Timeout) => return Err(UsbError::AddressDeviceTimeout),
                    Err(UsbError::CmdFail(code)) if code == completion::CONTEXT_STATE_ERROR => {
                        let diag = ctrl.command_diag_for_port(port);
                        let out_base = device_ctx.as_ptr::<u8>();
                        let out_slot_ctx = unsafe { *(out_base as *const SlotContext) };
                        let out_ep0_ctx =
                            unsafe { *(out_base.add(ctx_stride) as *const EndpointContext) };
                        let input_ctrl =
                            unsafe { core::slice::from_raw_parts(input_base as *const u32, 8) };
                        ctrl.emit_diag(
                            0x0318,
                            ((diag.usbcmd as u64) << 32) | diag.usbsts as u64,
                            ((diag.portsc as u64) << 32)
                                | ((code as u64) << 16)
                                | attempt_idx as u64,
                            ((slot_id as u64) << 32) | max_packet as u64,
                        );
                        ctrl.emit_diag(
                            0x0323,
                            port as u64,
                            encode_port_state(diag.portsc),
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                        );
                        ctrl.emit_diag(
                            0x0319,
                            diag.crcr,
                            diag.dcbaap,
                            ((diag.iman as u64) << 32) | (diag.portsc as u64),
                        );
                        ctrl.emit_diag(0x031a, diag.erdp, diag.erstba, 0);
                        let out_slot_state = ((out_slot_ctx.dw3 >> 27) & 0x1f) as u64;
                        let out_ep0_state = (out_ep0_ctx.dw0 & 0x7) as u64;
                        ctrl.emit_diag(
                            0x0324,
                            ((out_slot_state as u64) << 32) | out_ep0_state as u64,
                            ctrl.device_context_entry(slot_id),
                            ((diag.portsc as u64) << 32) | code as u64,
                        );
                        ctrl.emit_diag(
                            0x031b,
                            ((out_slot_ctx.dw0 as u64) << 32) | out_slot_ctx.dw1 as u64,
                            ((out_slot_ctx.dw2 as u64) << 32) | out_slot_ctx.dw3 as u64,
                            0,
                        );
                        ctrl.emit_diag(
                            0x031c,
                            ((out_ep0_ctx.dw0 as u64) << 32) | out_ep0_ctx.dw1 as u64,
                            ((out_ep0_ctx.tr_dequeue_hi as u64) << 32)
                                | out_ep0_ctx.tr_dequeue_lo as u64,
                            out_ep0_ctx.dw4 as u64,
                        );
                        ctrl.emit_diag(
                            0x031d,
                            ((input_ctrl[0] as u64) << 32) | input_ctrl[1] as u64,
                            ((input_ctrl[2] as u64) << 32) | input_ctrl[3] as u64,
                            ((input_ctrl[4] as u64) << 32) | input_ctrl[5] as u64,
                        );
                        ctrl.emit_diag(
                            0x031e,
                            ((input_ctrl[6] as u64) << 32) | input_ctrl[7] as u64,
                            input_ctx.phys(host),
                            device_ctx.phys(host),
                        );
                        last_ctx_state_err = Some(UsbError::CmdFail(code));
                        if attempt_idx + 1 < candidate_count {
                            continue;
                        }
                    }
                    Err(err) => return Err(err),
                }
            }

            if let Some(err) = last_ctx_state_err {
                if slot_recycles < MAX_SLOT_RECYCLES {
                    let old_slot = slot_id;
                    slot_recycles = slot_recycles.saturating_add(1);
                    ctrl.emit_diag(0x0313, old_slot as u64, slot_recycles as u64, 0);
                    ctrl.emit_diag(
                        0x0326,
                        old_slot as u64,
                        ctrl.device_context_entry(old_slot),
                        slot_recycles as u64,
                    );

                    ctrl.set_device_context(old_slot, 0);
                    let _ = ctrl.disable_slot(old_slot);

                    slot_id = match ctrl.enable_slot() {
                        Err(UsbError::Timeout) => return Err(UsbError::EnableSlotTimeout),
                        Err(enable_err) => return Err(enable_err),
                        Ok(new_slot) => new_slot,
                    };
                    unsafe {
                        core::ptr::write_bytes(device_ctx.as_ptr::<u8>(), 0, device_ctx_bytes);
                    }
                    ctrl.set_device_context(slot_id, device_ctx.phys(host));
                    compiler_fence(Ordering::Release);
                    ctrl.emit_diag(0x0314, old_slot as u64, slot_id as u64, 0);
                    ctrl.emit_diag(
                        0x0327,
                        ((old_slot as u64) << 32) | slot_id as u64,
                        ctrl.device_context_entry(old_slot),
                        ctrl.device_context_entry(slot_id),
                    );
                    continue;
                }
                return Err(err);
            }
            return Err(UsbError::CmdFail(completion::CONTEXT_STATE_ERROR));
        }

        // Allocate ep_rings on heap to reduce stack usage
        let mut ep_rings = Vec::with_capacity(31);
        ep_rings.resize_with(31, || None);

        Ok(Self {
            ctrl,
            slot_id,
            port,
            root_hub_port,
            route: route & ROUTE_STRING_MASK,
            speed,
            device_ctx,
            input_ctx,
            ep0_ring: Mutex::new(ep0_ring),
            ep_rings: Mutex::new(ep_rings),
            device_desc: None,
        })
    }

    /// Perform a control transfer
    pub fn control_transfer(
        &self,
        setup: &SetupPacket,
        data: Option<&mut [u8]>,
    ) -> Result<usize> {
        self.control_transfer_with_wait_spins(setup, data, CONTROL_XFER_WAIT_SPINS)
    }

    /// Perform a control transfer with a custom poll-spin wait budget.
    ///
    /// This is intended for callers that must avoid long stalls on optional or
    /// best-effort control requests (e.g. hub-class feature/status queries).
    pub fn control_transfer_with_wait_spins(
        &self,
        setup: &SetupPacket,
        mut data: Option<&mut [u8]>,
        wait_spins: usize,
    ) -> Result<usize> {
        let host = self.ctrl.host();
        let mut ep0_ring = self.ep0_ring.lock();
        let wait_limit = wait_spins.max(1);

        let data_dir = (setup.request_type & 0x80) != 0; // true = IN
        let data_len = data.as_ref().map(|d| d.len()).unwrap_or(0);
        self.ctrl.emit_diag(
            0x0339,
            ((setup.request_type as u64) << 56)
                | ((setup.request as u64) << 48)
                | ((setup.value as u64) << 32)
                | (setup.index as u64),
            ((setup.length as u64) << 48) | (data_len as u64),
            self.slot_id as u64,
        );

        // Allocate data buffer if needed
        // Use 64-byte alignment for DMA efficiency (cache line size)
        let data_buf = if data_len > 0 {
            let buf = PhysMem::alloc(host, data_len, 64)?;
            if !data_dir {
                // OUT: copy data to buffer
                if let Some(ref d) = data {
                    unsafe {
                        core::ptr::copy_nonoverlapping(d.as_ptr(), buf.as_ptr(), d.len());
                    }
                }
            }
            Some(buf)
        } else {
            None
        };

        let value = setup.value.to_le_bytes();
        let index = setup.index.to_le_bytes();
        let length = setup.length.to_le_bytes();
        let setup_immediate = u64::from_le_bytes([
            setup.request_type,
            setup.request,
            value[0],
            value[1],
            index[0],
            index[1],
            length[0],
            length[1],
        ]);

        // Setup Stage TRB
        let setup_trb = Trb {
            param: setup_immediate,
            status: 8, // Transfer length = 8
            control: (trb_type::SETUP << 10)
                | (1 << 6) // IDT (Immediate Data)
                | if data_len > 0 && setup.length > 0 {
                    if data_dir { 3 << 16 } else { 2 << 16 } // TRT: IN or OUT
                } else {
                    0 // No data stage
                },
        };
        let setup_trb_addr = ep0_ring.enqueue(host, setup_trb);

        // Data Stage TRB (if needed)
        let data_trb_addr = if let Some(ref buf) = data_buf {
            let data_trb = Trb {
                param: buf.phys(host),
                status: setup.length as u32,
                control: (trb_type::DATA << 10)
                    | if data_dir { 1 << 16 } else { 0 } // DIR
                    | (1 << 5), // IOC for debugging
            };
            Some(ep0_ring.enqueue(host, data_trb))
        } else {
            None
        };

        // Status Stage TRB
        let status_trb = Trb {
            param: 0,
            status: 0,
            control: (trb_type::STATUS << 10)
                | if data_len > 0 && setup.length > 0 && data_dir { 0 } else { 1 << 16 } // DIR
                | (1 << 5), // IOC
        };
        let status_trb_addr = ep0_ring.enqueue(host, status_trb);
        self.ctrl.emit_diag(
            0x0334,
            setup_trb_addr,
            data_trb_addr.unwrap_or(0),
            status_trb_addr,
        );

        drop(ep0_ring);

        // Ring doorbell for EP0 (target = 1)
        self.ctrl.ring_doorbell(self.slot_id, 1);

        // Wait for completion
        let mut waited = 0usize;
        let mut data_stage_remaining: Option<u32> = None;
        loop {
            if let Some(evt) = self.ctrl.poll_event()
                && evt.trb_type() == trb_type::TRANSFER_EVENT as u8
                && evt.slot_id() == self.slot_id
            {
                let completion_ptr = evt.param & !0x0f;
                let ep_id = evt.endpoint_id();
                let stage = if completion_ptr == setup_trb_addr {
                    1u8
                } else if data_trb_addr == Some(completion_ptr) {
                    2u8
                } else if completion_ptr == status_trb_addr {
                    3u8
                } else {
                    0u8
                };
                let code = evt.completion_code();
                self.ctrl.emit_diag(
                    0x0335,
                    completion_ptr,
                    ((code as u64) << 56)
                        | ((stage as u64) << 48)
                        | ((ep_id as u64) << 40)
                        | evt.transfer_length() as u64,
                    evt.control as u64,
                );
                if ep_id != 1 {
                    continue;
                }

                if stage == 1 {
                    continue;
                }

                if stage == 2 {
                    data_stage_remaining = Some(evt.transfer_length());
                    match code {
                        completion::SUCCESS | completion::SHORT_PACKET => continue,
                        completion::STALL_ERROR => {
                            self.recover_ep0_after_failure(completion_ptr, code, stage);
                            self.emit_control_failure_context(
                                setup,
                                data_len,
                                completion_ptr,
                                code,
                                stage,
                                waited,
                            );
                            if let Some(buf) = data_buf {
                                buf.free(host);
                            }
                            return Err(UsbError::Stall);
                        }
                        _ => {
                            self.recover_ep0_after_failure(completion_ptr, code, stage);
                            self.emit_control_failure_context(
                                setup,
                                data_len,
                                completion_ptr,
                                code,
                                stage,
                                waited,
                            );
                            if let Some(buf) = data_buf {
                                buf.free(host);
                            }
                            return Err(UsbError::XferFail(code));
                        }
                    }
                }

                if stage != 3 {
                    continue;
                }

                match code {
                    completion::SUCCESS | completion::SHORT_PACKET => {
                        let transferred = if data_len > 0 {
                            let remaining =
                                data_stage_remaining.unwrap_or_else(|| evt.transfer_length());
                            if data_stage_remaining.is_none() {
                                let mut sample = [0u8; 8];
                                let sample_len = core::cmp::min(sample.len(), data_len);
                                if let Some(buf) = &data_buf {
                                    unsafe {
                                        core::ptr::copy_nonoverlapping(
                                            buf.as_ptr::<u8>(),
                                            sample.as_mut_ptr(),
                                            sample_len,
                                        );
                                    }
                                }
                                self.ctrl.emit_diag(
                                    0x0336,
                                    ((setup.length as u64) << 32)
                                        | (evt.transfer_length() as u64),
                                    remaining as u64,
                                    u64::from_le_bytes(sample),
                                );
                            }
                            (setup.length as usize).saturating_sub(remaining as usize)
                        } else {
                            0
                        };

                        // Copy data back for IN transfers
                        if data_dir && let (Some(buf), Some(d)) = (&data_buf, &mut data) {
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    buf.as_ptr::<u8>(),
                                    d.as_mut_ptr(),
                                    transferred.min(d.len()),
                                );
                            }
                        }

                        if let Some(buf) = data_buf {
                            buf.free(host);
                        }
                        return Ok(transferred);
                    }
                    completion::STALL_ERROR => {
                        self.recover_ep0_after_failure(completion_ptr, code, stage);
                        self.emit_control_failure_context(
                            setup,
                            data_len,
                            completion_ptr,
                            code,
                            stage,
                            waited,
                        );
                        if let Some(buf) = data_buf {
                            buf.free(host);
                        }
                        return Err(UsbError::Stall);
                    }
                    _ => {
                        self.recover_ep0_after_failure(completion_ptr, code, stage);
                        self.emit_control_failure_context(
                            setup,
                            data_len,
                            completion_ptr,
                            code,
                            stage,
                            waited,
                        );
                        if let Some(buf) = data_buf {
                            buf.free(host);
                        }
                        return Err(UsbError::XferFail(code));
                    }
                }
            }
            waited = waited.saturating_add(1);
            if waited >= wait_limit {
                self.ctrl.emit_diag(
                    0x033a,
                    ((setup.request_type as u64) << 56)
                        | ((setup.request as u64) << 48)
                        | ((setup.value as u64) << 32)
                        | (setup.index as u64),
                    ((setup.length as u64) << 48) | (data_len as u64),
                    wait_limit as u64,
                );
                self.recover_ep0_after_failure(status_trb_addr, 0xff, 0);
                self.emit_control_failure_context(
                    setup,
                    data_len,
                    status_trb_addr,
                    0xff,
                    0,
                    waited,
                );
                if let Some(buf) = data_buf {
                    buf.free(host);
                }
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }
    }

    /// Get device descriptor
    pub fn get_device_descriptor(&mut self) -> Result<DeviceDesc> {
        let mut buf = [0u8; 18];
        let setup = SetupPacket::get_descriptor(desc_type::DEVICE, 0, 18);
        let transferred = self.control_transfer(&setup, Some(&mut buf))?;
        self.ctrl.emit_diag(
            0x0337,
            transferred as u64,
            u64::from_le_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
            u64::from_le_bytes([
                buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
            ]),
        );
        self.ctrl.emit_diag(
            0x0338,
            ((buf[16] as u64) << 8) | (buf[17] as u64),
            ((buf[0] as u64) << 56)
                | ((buf[1] as u64) << 48)
                | ((buf[2] as u64) << 40)
                | ((buf[3] as u64) << 32)
                | ((buf[4] as u64) << 24)
                | ((buf[5] as u64) << 16)
                | ((buf[6] as u64) << 8)
                | (buf[7] as u64),
            0,
        );

        let desc = unsafe { *(buf.as_ptr() as *const DeviceDesc) };
        self.device_desc = Some(desc);
        Ok(desc)
    }

    /// Get configuration descriptor (full, with interfaces and endpoints)
    pub fn get_config_descriptor(&self, index: u8) -> Result<Vec<u8>> {
        // First, read just the configuration header to discover total length.
        let mut header = [0u8; CONFIG_DESC_MIN_LEN];
        for attempt in 0..CONFIG_DESC_HEADER_RETRIES {
            let setup = SetupPacket::get_descriptor(
                desc_type::CONFIGURATION,
                index,
                CONFIG_DESC_MIN_LEN as u16,
            );
            let header_xfer = self.control_transfer(&setup, Some(&mut header))?;
            let total_len = u16::from_le_bytes([header[2], header[3]]) as usize;
            let header_lo = u64::from_le_bytes([
                header[0], header[1], header[2], header[3], header[4], header[5], header[6],
                header[7],
            ]);
            self.ctrl.emit_diag(
                0x0330,
                ((attempt as u64) << 32) | (header_xfer as u64),
                header_lo,
                ((header[8] as u64) << 56)
                    | ((header[0] as u64) << 48)
                    | ((header[1] as u64) << 40)
                    | ((total_len as u64) & 0xffff_ffff),
            );

            let header_valid = header_xfer >= 4
                && header[0] as usize >= CONFIG_DESC_MIN_LEN
                && header[1] == desc_type::CONFIGURATION
                && (CONFIG_DESC_MIN_LEN..=CONFIG_DESC_MAX_LEN).contains(&total_len);
            if !header_valid {
                self.ctrl.emit_diag(
                    0x0331,
                    ((attempt as u64) << 32) | (header_xfer as u64),
                    total_len as u64,
                    ((header[0] as u64) << 8) | (header[1] as u64),
                );
                if attempt + 1 < CONFIG_DESC_HEADER_RETRIES {
                    continue;
                }
                return Err(UsbError::InvalidDescriptor);
            }

            // Now read the reported configuration payload.
            let mut full_buf = alloc::vec![0u8; total_len];
            let setup = SetupPacket::get_descriptor(desc_type::CONFIGURATION, index, total_len as u16);
            let full_xfer = self.control_transfer(&setup, Some(&mut full_buf))?;
            self.ctrl.emit_diag(
                0x0332,
                ((full_xfer as u64) << 32) | (total_len as u64),
                full_buf
                    .get(0)
                    .copied()
                    .map_or(0, |b0| (b0 as u64) << 56)
                    | full_buf
                        .get(1)
                        .copied()
                        .map_or(0, |b1| (b1 as u64) << 48)
                    | full_buf
                        .get(2)
                        .copied()
                        .map_or(0, |b2| (b2 as u64) << 40)
                    | full_buf
                        .get(3)
                        .copied()
                        .map_or(0, |b3| (b3 as u64) << 32),
                ((header[0] as u64) << 24) | ((header[1] as u64) << 16) | (index as u64),
            );
            if full_xfer < CONFIG_DESC_MIN_LEN {
                self.ctrl.emit_diag(
                    0x0333,
                    ((attempt as u64) << 32) | (full_xfer as u64),
                    total_len as u64,
                    0,
                );
                if attempt + 1 < CONFIG_DESC_HEADER_RETRIES {
                    continue;
                }
                return Err(UsbError::InvalidDescriptor);
            }
            full_buf.truncate(full_xfer);
            return Ok(full_buf);
        }

        Err(UsbError::InvalidDescriptor)
    }

    /// Set configuration
    pub fn set_configuration(&self, config: u8) -> Result<()> {
        let setup = SetupPacket::set_configuration(config);
        self.control_transfer(&setup, None)?;
        Ok(())
    }

    /// Set hub depth for a configured SuperSpeed hub.
    ///
    /// `depth` is the number of external hubs between root hub and this hub.
    pub fn set_hub_depth(&self, depth: u8) -> Result<()> {
        let setup = SetupPacket::hub_set_depth(depth);
        self.control_transfer(&setup, None)?;
        Ok(())
    }

    /// Configure an endpoint (after SET_CONFIGURATION)
    pub fn configure_endpoint(&self, ep: &EndpointDesc) -> Result<()> {
        let host = self.ctrl.host();

        let ep_num = ep.number();
        let is_in = ep.is_in();
        let ep_type = ep.transfer_type();

        // Endpoint Context Index: EP1 OUT = 2, EP1 IN = 3, EP2 OUT = 4, etc.
        let dci = (ep_num as usize * 2) + if is_in { 1 } else { 0 };
        let ring_idx = dci - 1; // rings array is 0-indexed for EP1+

        // Allocate transfer ring for this endpoint
        let ring = Ring::new(host, 256)?;
        let ring_phys = ring.phys(host);

        // Update input context
        unsafe {
            *self.input_drop_flags_ptr() = 0; // Drop flags
            *self.input_add_flags_ptr() = (1 << dci) | 1; // Add flags: this EP + Slot

            // xHCI endpoint type encoding
            let xhci_ep_type = match (ep_type, is_in) {
                (0, _) => 4,     // Control (bidirectional)
                (1, false) => 1, // Isoch OUT
                (1, true) => 5,  // Isoch IN
                (2, false) => 2, // Bulk OUT
                (2, true) => 6,  // Bulk IN
                (3, false) => 3, // Interrupt OUT
                (3, true) => 7,  // Interrupt IN
                _ => 4,
            };

            // Calculate interval for xHCI (different from USB descriptor)
            let interval = if self.speed >= reg::SPEED_HIGH {
                ep.interval.saturating_sub(1)
            } else {
                // For FS/LS, convert ms to 125us frames
                // Use integer log2: find highest set bit
                let ms = ep.interval.max(1) as u32;
                let log2_ceil = if ms.is_power_of_two() {
                    ms.trailing_zeros() as u8
                } else {
                    (u32::BITS - ms.leading_zeros()) as u8
                };
                log2_ceil + 3
            };

            *self.input_ep_ctx_ptr(ring_idx) =
                EndpointContext::new(xhci_ep_type, ep.max_packet_size, 0, interval, ring_phys);
        }

        // Store ring
        let mut ep_rings = self.ep_rings.lock();
        ep_rings[ring_idx] = Some(ring);
        drop(ep_rings);

        // Configure Endpoint command
        let trb = Trb {
            param: self.input_ctx.phys(host),
            status: 0,
            control: (trb_type::CONFIGURE_ENDPOINT << 10) | ((self.slot_id as u32) << 24),
        };
        compiler_fence(Ordering::Release);
        self.ctrl.submit_command(trb)?;

        Ok(())
    }

    /// Queue a transfer on an endpoint
    pub fn queue_transfer(
        &self,
        ep_num: u8,
        is_in: bool,
        buf: &PhysMem<H>,
        len: usize,
    ) -> Result<()> {
        let dci = (ep_num as usize * 2) + if is_in { 1 } else { 0 };
        let ring_idx = dci - 1;

        let mut ep_rings = self.ep_rings.lock();
        let ring = ep_rings[ring_idx].as_mut().ok_or(UsbError::InvEndpoint)?;

        let host = self.ctrl.host();
        let trb = Trb {
            param: buf.phys(host),
            status: len as u32,
            control: (trb_type::NORMAL << 10) | (1 << 5), // IOC
        };
        ring.enqueue(host, trb);
        drop(ep_rings);

        // Ring doorbell
        self.ctrl.ring_doorbell(self.slot_id, dci as u8);

        Ok(())
    }

    /// Returns the xHCI slot ID assigned to this device.
    pub fn slot_id(&self) -> u8 {
        self.slot_id
    }

    /// Returns the root hub port index (0-based) this device routes through.
    pub fn port(&self) -> u8 {
        self.port
    }

    /// Returns the root hub port number (USB numbering, starts at 1).
    pub fn root_hub_port(&self) -> u8 {
        self.root_hub_port
    }

    /// Returns the xHCI route string (20-bit).
    pub fn route(&self) -> u32 {
        self.route
    }

    /// Returns the device speed (see `reg::SPEED_*` constants).
    pub fn speed(&self) -> u8 {
        self.speed
    }

    /// Returns a reference to the xHCI controller.
    pub fn ctrl(&self) -> &Arc<XhciCtrl<H>> {
        &self.ctrl
    }
}

impl<H: Dma> Drop for UsbDevice<H> {
    fn drop(&mut self) {
        let _ = self.ctrl.disable_slot(self.slot_id);

        let host = self.ctrl.host();

        // Free endpoint rings
        let mut ep_rings = self.ep_rings.lock();
        for ring in ep_rings.iter_mut() {
            if let Some(r) = ring.take() {
                r.free(host);
            }
        }
        drop(ep_rings);

        // Free EP0 ring
        let ep0_ring = core::mem::replace(&mut *self.ep0_ring.lock(), Ring::new(host, 2).unwrap());
        ep0_ring.free(host);
    }
}
