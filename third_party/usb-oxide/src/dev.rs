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
const SLOT_DEV_HUB: u32 = 1 << 26;
const SLOT_NUM_PORTS_SHIFT: u32 = 24;
const SLOT_NUM_PORTS_MASK: u32 = 0xff << SLOT_NUM_PORTS_SHIFT;
const TT_PORT_SHIFT: u32 = 8;
const TT_THINK_TIME_SHIFT: u32 = 16;
const CONTEXT_ALIGN_BYTES: usize = 64;
const CONFIG_DESC_MIN_LEN: usize = 9;
const CONFIG_DESC_MAX_LEN: usize = 4096;
const CONFIG_DESC_HEADER_RETRIES: usize = 3;
const MAX_ENDPOINT_CONTEXTS: usize = 31;
const DEVICE_CONTEXT_ENTRIES: usize = 1 + MAX_ENDPOINT_CONTEXTS;
const INPUT_CONTEXT_ENTRIES: usize = 2 + MAX_ENDPOINT_CONTEXTS;
const ADDRESS_RETRY_PATH_CLAMP_TT: u8 = 1;
const ADDRESS_RETRY_PATH_SINGLE_TT: u8 = 2;
const ADDRESS_RETRY_PATH_REDUCE_TTT: u8 = 3;
const ADDRESS_RETRY_PATH_KEEP_TT_RECYCLE: u8 = 4;
const ADDRESS_RETRY_PATH_CONTEXT_STATE: u8 = 5;
const ADDRESS_RETRY_PATH_DIRECT_FAIL: u8 = 6;
const ADDRESS_RETRY_PATH_DROP_TT_CONTEXT: u8 = 7;

#[inline]
const fn encode_retry_state(
    path: u8,
    code: u8,
    clamp_used: bool,
    single_used: bool,
    reduce_count: usize,
    keep_tt_used: bool,
) -> u64 {
    ((path as u64) << 56)
        | ((code as u64) << 48)
        | ((clamp_used as u64) << 40)
        | ((single_used as u64) << 32)
        | ((reduce_count as u64) << 16)
        | (keep_tt_used as u64)
}

#[inline]
const fn encode_tt_info(tt: TtContext) -> u32 {
    let tt_think_time = (tt.tt_think_time & 0x03) as u32;
    (tt.hub_slot_id as u32)
        | ((tt.downstream_port as u32) << TT_PORT_SHIFT)
        | (tt_think_time << TT_THINK_TIME_SHIFT)
}

#[inline]
const fn encode_address_tt_info(tt: TtContext) -> u32 {
    let port = if tt.downstream_port == 0 {
        1
    } else {
        tt.downstream_port
    };
    (tt.hub_slot_id as u32) | ((port as u32) << TT_PORT_SHIFT)
}

#[inline]
const fn clamp_tt_context(tt: TtContext) -> TtContext {
    let tt_think_time = match tt.tt_think_time & 0x03 {
        0 => 0u8,
        1 => 1u8,
        _ => 2u8,
    };
    TtContext {
        hub_slot_id: tt.hub_slot_id,
        downstream_port: tt.downstream_port,
        tt_think_time,
        multi_tt: tt.multi_tt,
    }
}

#[inline]
const fn single_tt_profile(tt: TtContext) -> TtContext {
    let clamped = clamp_tt_context(tt);
    TtContext {
        hub_slot_id: clamped.hub_slot_id,
        // Keep physical downstream port numbering; Linux uses udev->ttport
        // even when MTT is clear.
        downstream_port: if clamped.downstream_port == 0 {
            1
        } else {
            clamped.downstream_port
        },
        tt_think_time: clamped.tt_think_time,
        multi_tt: false,
    }
}

#[inline]
const fn canonicalize_tt_context(tt: TtContext) -> TtContext {
    let base = if tt.multi_tt {
        clamp_tt_context(tt)
    } else {
        single_tt_profile(tt)
    };
    // Linux programs TT slot/port for Address Device and leaves TT think time
    // clear in child slot contexts; mirror that model for Pi4/VL805.
    TtContext {
        hub_slot_id: base.hub_slot_id,
        downstream_port: base.downstream_port,
        tt_think_time: 0,
        multi_tt: base.multi_tt,
    }
}

#[inline]
const fn reduced_tt_think_time_profile(tt: TtContext) -> Option<TtContext> {
    let clamped = clamp_tt_context(tt);
    if clamped.tt_think_time == 0 {
        None
    } else {
        Some(TtContext {
            hub_slot_id: clamped.hub_slot_id,
            downstream_port: clamped.downstream_port,
            tt_think_time: clamped.tt_think_time - 1,
            multi_tt: clamped.multi_tt,
        })
    }
}

#[inline]
const fn slot_ctx_with_hub_info(
    mut slot_ctx: SlotContext,
    speed: u8,
    num_ports: u8,
    multi_tt: bool,
) -> SlotContext {
    slot_ctx.dw0 |= SLOT_DEV_HUB;
    if speed == reg::SPEED_HIGH && multi_tt {
        slot_ctx.dw0 |= SLOT_DEV_MTT;
    } else {
        slot_ctx.dw0 &= !SLOT_DEV_MTT;
    }
    slot_ctx.dw1 =
        (slot_ctx.dw1 & !SLOT_NUM_PORTS_MASK) | ((num_ports as u32) << SLOT_NUM_PORTS_SHIFT);
    slot_ctx
}

#[inline]
const fn should_retry_with_clamped_tt(
    completion_code: u8,
    tt_ctx: Option<TtContext>,
    already_retried: bool,
) -> bool {
    if completion_code != completion::PARAMETER_ERROR || already_retried {
        return false;
    }
    match tt_ctx {
        Some(tt) => (tt.tt_think_time & 0x03) > 2,
        None => false,
    }
}

#[inline]
const fn should_retry_with_single_tt_profile(
    completion_code: u8,
    tt_ctx: Option<TtContext>,
    already_retried: bool,
) -> bool {
    if completion_code != completion::PARAMETER_ERROR || already_retried {
        return false;
    }
    match tt_ctx {
        Some(tt) => tt.multi_tt,
        None => false,
    }
}

#[inline]
const fn should_retry_with_reduced_tt_think_time(
    completion_code: u8,
    tt_ctx: Option<TtContext>,
    reductions_applied: usize,
) -> bool {
    if completion_code != completion::PARAMETER_ERROR || reductions_applied >= 2 {
        return false;
    }
    match tt_ctx {
        Some(tt) => clamp_tt_context(tt).tt_think_time > 0,
        None => false,
    }
}

#[inline]
const fn should_retry_without_tt_context(
    completion_code: u8,
    tt_ctx: Option<TtContext>,
    already_retried: bool,
) -> bool {
    completion_code == completion::PARAMETER_ERROR && tt_ctx.is_some() && !already_retried
}

#[inline]
const fn should_retry_by_dropping_tt_context(
    completion_code: u8,
    tt_ctx: Option<TtContext>,
    reductions_applied: usize,
    already_retried: bool,
) -> bool {
    if already_retried || tt_ctx.is_none() {
        return false;
    }
    completion_code == completion::PARAMETER_ERROR && reductions_applied >= 2
}

#[inline]
const fn should_attempt_ep0_hardware_recovery(completion_code: u8, stage: u8) -> bool {
    let _ = completion_code;
    let _ = stage;
    false
}

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
    /// Hub TT think time (0=8, 1=16, 2=24, 3=32 FS bit times).
    pub tt_think_time: u8,
    /// Whether the parent hub advertises Multi-TT support.
    pub multi_tt: bool,
}

/// Ensures a slot enabled during probe is cleaned up on early-return errors.
struct SlotCleanup<H: Dma> {
    ctrl: Arc<XhciCtrl<H>>,
    slot_id: u8,
    armed: bool,
}

impl<H: Dma> SlotCleanup<H> {
    fn new(ctrl: Arc<XhciCtrl<H>>, slot_id: u8) -> Self {
        Self {
            ctrl,
            slot_id,
            armed: true,
        }
    }

    fn set_slot(&mut self, slot_id: u8) {
        self.slot_id = slot_id;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl<H: Dma> Drop for SlotCleanup<H> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.ctrl.set_device_context(self.slot_id, 0);
        let _ = self.ctrl.disable_slot(self.slot_id);
    }
}

fn recycle_slot_after_address_failure<H: Dma>(
    ctrl: &Arc<XhciCtrl<H>>,
    slot_cleanup: &mut SlotCleanup<H>,
    slot_id: &mut u8,
    device_ctx: &PhysMem<H>,
    device_ctx_bytes: usize,
    slot_recycles: &mut usize,
    max_slot_recycles: usize,
) -> Result<bool> {
    if *slot_recycles >= max_slot_recycles {
        return Ok(false);
    }

    let old_slot = *slot_id;
    *slot_recycles = slot_recycles.saturating_add(1);
    ctrl.emit_diag(0x0383, old_slot as u64, *slot_recycles as u64, 0);
    ctrl.emit_diag(
        0x0396,
        old_slot as u64,
        ctrl.device_context_entry(old_slot),
        *slot_recycles as u64,
    );

    ctrl.set_device_context(old_slot, 0);
    let _ = ctrl.disable_slot(old_slot);

    *slot_id = match ctrl.enable_slot() {
        Err(UsbError::Timeout) => return Err(UsbError::EnableSlotTimeout),
        Err(enable_err) => return Err(enable_err),
        Ok(new_slot) => new_slot,
    };
    slot_cleanup.set_slot(*slot_id);

    // SAFETY: `device_ctx` owns a DMA allocation of `device_ctx_bytes` bytes,
    // and this reset happens before the context is republished to the xHC.
    unsafe {
        core::ptr::write_bytes(device_ctx.as_ptr::<u8>(), 0, device_ctx_bytes);
    }
    ctrl.set_device_context(*slot_id, device_ctx.phys(ctrl.host()));
    compiler_fence(Ordering::Release);

    ctrl.emit_diag(0x0384, old_slot as u64, *slot_id as u64, 0);
    ctrl.emit_diag(
        0x0397,
        ((old_slot as u64) << 32) | *slot_id as u64,
        ctrl.device_context_entry(old_slot),
        ctrl.device_context_entry(*slot_id),
    );
    Ok(true)
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
            0x03ab,
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
            0x03ac,
            ((setup.length as u64) << 48) | (data_len as u64),
            self.ctrl.device_context_entry(self.slot_id),
            ((ep0_ctx.tr_dequeue_hi as u64) << 32) | ep0_ctx.tr_dequeue_lo as u64,
        );
        self.ctrl.emit_diag(
            0x03ad,
            ((slot_ctx.dw0 as u64) << 32) | slot_ctx.dw1 as u64,
            ((slot_ctx.dw2 as u64) << 32) | slot_ctx.dw3 as u64,
            ((ep0_ctx.dw0 as u64) << 32) | ep0_ctx.dw1 as u64,
        );
        self.ctrl.emit_diag(
            0x03ae,
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
            0x03af,
            completion_ptr,
            ((completion_code as u64) << 56)
                | ((stage as u64) << 48)
                | ((self.slot_id as u64) << 40)
                | (1u64 << 32) // EP0 xHCI endpoint ID
                | enqueue_idx as u64,
            ((producer_cycle as u64) << 63) | dequeue_ptr,
        );

        // EP0 recovers on the next SETUP stage. Reset Endpoint / Set TR Dequeue
        // is appropriate for halted data endpoints, but it can poison cascaded
        // hub control traffic on Pi4/VL805 after an ordinary control timeout or
        // transfer error.
        if !should_attempt_ep0_hardware_recovery(completion_code, stage) {
            self.ctrl.emit_diag(
                0x03b0,
                ((self.slot_id as u64) << 32) | 1u64,
                dequeue_ptr,
                4u64 << 32,
            );
            return;
        }

        let reset_result = self.ctrl.reset_endpoint(self.slot_id, 1);
        self.ctrl.emit_diag(
            0x03b0,
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
            0x03b1,
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
        let tt_ctx = tt_context.map(canonicalize_tt_context);

        // Enable slot
        let mut slot_id = ctrl.enable_slot()?;
        let mut slot_cleanup = SlotCleanup::new(ctrl.clone(), slot_id);
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
        let build_slot_ctx = |slot_speed: u8, slot_tt_ctx: Option<TtContext>| {
                let mut slot_ctx = SlotContext::new(route, slot_speed, 1, root_hub_port);
                if let Some(tt) = slot_tt_ctx {
                    if tt.multi_tt {
                        slot_ctx.dw0 |= SLOT_DEV_MTT;
                    }
                    slot_ctx.dw2 = encode_address_tt_info(tt);
                }
                slot_ctx
            };

        // Set device context in DCBAA
        // SAFETY: Both context buffers are owned DMA allocations sized by the
        // validated xHCI context stride; zeroing them happens before publication.
        unsafe {
            core::ptr::write_bytes(device_ctx.as_ptr::<u8>(), 0, device_ctx_bytes);
            core::ptr::write_bytes(input_base, 0, input_ctx_bytes);
        }
        ctrl.set_device_context(slot_id, device_ctx.phys(host));
        compiler_fence(Ordering::Release);
        ctrl.emit_diag(
            0x0390,
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

        const MAX_SLOT_RECYCLES: usize = 3;
        let mut slot_recycles = 0usize;
        let mut active_tt_ctx = tt_ctx;
        let mut tt_clamp_fallback_used = false;
        let mut tt_single_profile_fallback_used = false;
        let mut tt_think_time_reductions = 0usize;
        let mut tt_parameter_retry_used = false;
        let mut tt_drop_context_retry_used = false;
        let mut context_state_err_count = 0usize;
        let mut parameter_err_count = 0usize;
        'address_attempt: loop {
            let mut last_ctx_state_err: Option<UsbError> = None;
            for (attempt_idx, max_packet) in ep0_mps_candidates[..candidate_count]
                .iter()
                .copied()
                .enumerate()
            {
                let slot_ctx_template = build_slot_ctx(enumerated_speed, active_tt_ctx);
                // SAFETY: `device_ctx` and `input_ctx` are owned DMA allocations sized
                // above for their context layouts; all pointer writes stay within those
                // allocations and publish a clean Address Device input context.
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
                // SAFETY: The preceding context writes initialized the Slot and EP0
                // entries within `input_ctx`; `ctx_stride` was derived from the
                // controller context-size bit and the reads are by-value snapshots.
                let slot_ctx = unsafe { *(input_base.add(ctx_stride) as *const SlotContext) };
                // SAFETY: Same initialized input-context allocation as `slot_ctx`;
                // EP0 lives at context index 2 by xHCI layout.
                let ep0_ctx =
                    unsafe { *(input_base.add(ctx_stride * 2) as *const EndpointContext) };
                ctrl.emit_diag(
                    0x0380,
                    ((attempt_idx as u64) << 32) | enumerated_speed as u64,
                    ((slot_id as u64) << 32) | max_packet as u64,
                    slot_recycles as u64,
                );
                ctrl.emit_diag(
                    0x0381,
                    ((slot_ctx.dw0 as u64) << 32) | slot_ctx.dw1 as u64,
                    ((slot_ctx.dw2 as u64) << 32) | slot_ctx.dw3 as u64,
                    0,
                );
                ctrl.emit_diag(
                    0x0382,
                    ((ep0_ctx.dw0 as u64) << 32) | ep0_ctx.dw1 as u64,
                    ((ep0_ctx.tr_dequeue_hi as u64) << 32) | ep0_ctx.tr_dequeue_lo as u64,
                    ep0_ctx.dw4 as u64,
                );
                // SAFETY: The Input Control Context is the first eight u32 values
                // of the owned input-context allocation and was initialized above.
                let input_ctrl = unsafe { core::slice::from_raw_parts(input_base as *const u32, 8) };
                let portsc_before_addr = ctrl.port_status(port);
                ctrl.emit_diag(
                    0x0385,
                    input_ctx.phys(host),
                    device_ctx.phys(host),
                    ep0_ring.phys(host),
                );
                ctrl.emit_diag(
                    0x0386,
                    ((input_ctrl[0] as u64) << 32) | input_ctrl[1] as u64,
                    ((input_ctrl[2] as u64) << 32) | input_ctrl[3] as u64,
                    ((input_ctrl[4] as u64) << 32) | input_ctrl[5] as u64,
                );
                ctrl.emit_diag(
                    0x0387,
                    ((input_ctrl[6] as u64) << 32) | input_ctrl[7] as u64,
                    ((portsc_before_addr as u64) << 32) | slot_id as u64,
                    ((route as u64) << 32)
                        | ((root_hub_port as u64) << 16)
                        | ((enumerated_speed as u64) << 8)
                        | port as u64,
                );
                ctrl.emit_diag(
                    0x0392,
                    port as u64,
                    encode_port_state(portsc_before_addr),
                    ((slot_id as u64) << 32) | attempt_idx as u64,
                );
                ctrl.emit_diag(
                    0x0391,
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
                    Err(UsbError::CmdFail(code))
                        if should_retry_with_clamped_tt(
                            code,
                            active_tt_ctx,
                            tt_clamp_fallback_used,
                        ) =>
                    {
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x039f,
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                            ((slot_recycles as u64) << 48)
                                | ((max_packet as u64) << 32)
                                | tt_encode as u64,
                            encode_retry_state(
                                ADDRESS_RETRY_PATH_CLAMP_TT,
                                code,
                                tt_clamp_fallback_used,
                                tt_single_profile_fallback_used,
                                tt_think_time_reductions,
                                tt_parameter_retry_used,
                            ),
                        );
                        parameter_err_count = parameter_err_count.saturating_add(1);
                        if let Some(tt) = active_tt_ctx {
                            let clamped_tt = clamp_tt_context(tt);
                            ctrl.emit_diag(
                                0x0398,
                                ((slot_id as u64) << 32) | attempt_idx as u64,
                                encode_tt_info(tt) as u64,
                                ((encode_tt_info(clamped_tt) as u64) << 32)
                                    | ((max_packet as u64) << 16)
                                    | code as u64,
                            );
                            active_tt_ctx = Some(clamped_tt);
                        }
                        tt_clamp_fallback_used = true;
                        if !recycle_slot_after_address_failure(
                            &ctrl,
                            &mut slot_cleanup,
                            &mut slot_id,
                            &device_ctx,
                            device_ctx_bytes,
                            &mut slot_recycles,
                            MAX_SLOT_RECYCLES,
                        )? {
                            return Err(UsbError::CmdFail(code));
                        }
                        continue 'address_attempt;
                    }
                    Err(UsbError::CmdFail(code))
                        if should_retry_with_single_tt_profile(
                            code,
                            active_tt_ctx,
                            tt_single_profile_fallback_used,
                        ) =>
                    {
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x039f,
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                            ((slot_recycles as u64) << 48)
                                | ((max_packet as u64) << 32)
                                | tt_encode as u64,
                            encode_retry_state(
                                ADDRESS_RETRY_PATH_SINGLE_TT,
                                code,
                                tt_clamp_fallback_used,
                                tt_single_profile_fallback_used,
                                tt_think_time_reductions,
                                tt_parameter_retry_used,
                            ),
                        );
                        parameter_err_count = parameter_err_count.saturating_add(1);
                        if let Some(tt) = active_tt_ctx {
                            let single_tt = single_tt_profile(tt);
                            ctrl.emit_diag(
                                0x039a,
                                ((slot_id as u64) << 32) | attempt_idx as u64,
                                encode_tt_info(tt) as u64,
                                ((encode_tt_info(single_tt) as u64) << 32)
                                    | ((max_packet as u64) << 16)
                                    | code as u64,
                            );
                            active_tt_ctx = Some(single_tt);
                        }
                        tt_single_profile_fallback_used = true;
                        if !recycle_slot_after_address_failure(
                            &ctrl,
                            &mut slot_cleanup,
                            &mut slot_id,
                            &device_ctx,
                            device_ctx_bytes,
                            &mut slot_recycles,
                            MAX_SLOT_RECYCLES,
                        )? {
                            return Err(UsbError::CmdFail(code));
                        }
                        continue 'address_attempt;
                    }
                    Err(UsbError::CmdFail(code))
                        if should_retry_with_reduced_tt_think_time(
                            code,
                            active_tt_ctx,
                            tt_think_time_reductions,
                        ) =>
                    {
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x039f,
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                            ((slot_recycles as u64) << 48)
                                | ((max_packet as u64) << 32)
                                | tt_encode as u64,
                            encode_retry_state(
                                ADDRESS_RETRY_PATH_REDUCE_TTT,
                                code,
                                tt_clamp_fallback_used,
                                tt_single_profile_fallback_used,
                                tt_think_time_reductions,
                                tt_parameter_retry_used,
                            ),
                        );
                        parameter_err_count = parameter_err_count.saturating_add(1);
                        if let Some(tt) = active_tt_ctx {
                            if let Some(reduced_tt) = reduced_tt_think_time_profile(tt) {
                                ctrl.emit_diag(
                                    0x039e,
                                    ((slot_id as u64) << 32) | attempt_idx as u64,
                                    encode_tt_info(tt) as u64,
                                    ((encode_tt_info(reduced_tt) as u64) << 32)
                                        | ((tt_think_time_reductions as u64) << 16)
                                        | code as u64,
                                );
                                active_tt_ctx = Some(reduced_tt);
                                tt_think_time_reductions =
                                    tt_think_time_reductions.saturating_add(1);
                            }
                        }
                        if !recycle_slot_after_address_failure(
                            &ctrl,
                            &mut slot_cleanup,
                            &mut slot_id,
                            &device_ctx,
                            device_ctx_bytes,
                            &mut slot_recycles,
                            MAX_SLOT_RECYCLES,
                        )? {
                            return Err(UsbError::CmdFail(code));
                        }
                        continue 'address_attempt;
                    }
                    Err(UsbError::CmdFail(code))
                        if should_retry_without_tt_context(
                            code,
                            active_tt_ctx,
                            tt_parameter_retry_used,
                        ) =>
                    {
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x039f,
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                            ((slot_recycles as u64) << 48)
                                | ((max_packet as u64) << 32)
                                | tt_encode as u64,
                            encode_retry_state(
                                ADDRESS_RETRY_PATH_KEEP_TT_RECYCLE,
                                code,
                                tt_clamp_fallback_used,
                                tt_single_profile_fallback_used,
                                tt_think_time_reductions,
                                tt_parameter_retry_used,
                            ),
                        );
                        parameter_err_count = parameter_err_count.saturating_add(1);
                        if let Some(tt) = active_tt_ctx {
                            ctrl.emit_diag(
                                0x039d,
                                ((slot_id as u64) << 32) | attempt_idx as u64,
                                encode_tt_info(tt) as u64,
                                ((max_packet as u64) << 32)
                                    | ((slot_recycles as u64) << 16)
                                    | code as u64,
                            );
                        }
                        // Keep TT context intact for FS/LS devices behind HS hubs.
                        // Retrying after slot recycle without TT can provoke
                        // context-state failures on Pi4/vl805.
                        tt_parameter_retry_used = true;
                        if !recycle_slot_after_address_failure(
                            &ctrl,
                            &mut slot_cleanup,
                            &mut slot_id,
                            &device_ctx,
                            device_ctx_bytes,
                            &mut slot_recycles,
                            MAX_SLOT_RECYCLES,
                        )? {
                            return Err(UsbError::CmdFail(code));
                        }
                        continue 'address_attempt;
                    }
                    Err(UsbError::CmdFail(code))
                        if should_retry_by_dropping_tt_context(
                            code,
                            active_tt_ctx,
                            tt_think_time_reductions,
                            tt_drop_context_retry_used,
                        ) =>
                    {
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x039f,
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                            ((slot_recycles as u64) << 48)
                                | ((max_packet as u64) << 32)
                                | tt_encode as u64,
                            encode_retry_state(
                                ADDRESS_RETRY_PATH_DROP_TT_CONTEXT,
                                code,
                                tt_clamp_fallback_used,
                                tt_single_profile_fallback_used,
                                tt_think_time_reductions,
                                tt_parameter_retry_used,
                            ),
                        );
                        if let Some(tt) = active_tt_ctx {
                            ctrl.emit_diag(
                                0x0399,
                                ((slot_id as u64) << 32) | attempt_idx as u64,
                                encode_tt_info(tt) as u64,
                                ((max_packet as u64) << 32)
                                    | ((slot_recycles as u64) << 16)
                                    | code as u64,
                            );
                        }
                        active_tt_ctx = None;
                        tt_drop_context_retry_used = true;
                        if !recycle_slot_after_address_failure(
                            &ctrl,
                            &mut slot_cleanup,
                            &mut slot_id,
                            &device_ctx,
                            device_ctx_bytes,
                            &mut slot_recycles,
                            MAX_SLOT_RECYCLES,
                        )? {
                            return Err(UsbError::CmdFail(code));
                        }
                        continue 'address_attempt;
                    }
                    Err(UsbError::CmdFail(code)) if code == completion::CONTEXT_STATE_ERROR => {
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x039f,
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                            ((slot_recycles as u64) << 48)
                                | ((max_packet as u64) << 32)
                                | tt_encode as u64,
                            encode_retry_state(
                                ADDRESS_RETRY_PATH_CONTEXT_STATE,
                                code,
                                tt_clamp_fallback_used,
                                tt_single_profile_fallback_used,
                                tt_think_time_reductions,
                                tt_parameter_retry_used,
                            ),
                        );
                        context_state_err_count = context_state_err_count.saturating_add(1);
                        let diag = ctrl.command_diag_for_port(port);
                        let out_base = device_ctx.as_ptr::<u8>();
                        // SAFETY: Address Device completion has written the output
                        // context allocation; these are by-value diagnostic snapshots
                        // of the Slot/EP0 entries and initialized input-control words.
                        let out_slot_ctx = unsafe { *(out_base as *const SlotContext) };
                        // SAFETY: Same initialized output-context allocation as
                        // `out_slot_ctx`; EP0 lives at context index 1.
                        let out_ep0_ctx =
                            unsafe { *(out_base.add(ctx_stride) as *const EndpointContext) };
                        // SAFETY: The first eight u32 values are the initialized
                        // Input Control Context for this retry.
                        let input_ctrl =
                            unsafe { core::slice::from_raw_parts(input_base as *const u32, 8) };
                        ctrl.emit_diag(
                            0x0388,
                            ((diag.usbcmd as u64) << 32) | diag.usbsts as u64,
                            ((diag.portsc as u64) << 32)
                                | ((code as u64) << 16)
                                | attempt_idx as u64,
                            ((slot_id as u64) << 32) | max_packet as u64,
                        );
                        ctrl.emit_diag(
                            0x0393,
                            port as u64,
                            encode_port_state(diag.portsc),
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                        );
                        ctrl.emit_diag(
                            0x0389,
                            diag.crcr,
                            diag.dcbaap,
                            ((diag.iman as u64) << 32) | (diag.portsc as u64),
                        );
                        ctrl.emit_diag(0x038a, diag.erdp, diag.erstba, 0);
                        let out_slot_state = ((out_slot_ctx.dw3 >> 27) & 0x1f) as u64;
                        let out_ep0_state = (out_ep0_ctx.dw0 & 0x7) as u64;
                        ctrl.emit_diag(
                            0x0394,
                            ((out_slot_state as u64) << 32) | out_ep0_state as u64,
                            ctrl.device_context_entry(slot_id),
                            ((diag.portsc as u64) << 32) | code as u64,
                        );
                        ctrl.emit_diag(
                            0x038b,
                            ((out_slot_ctx.dw0 as u64) << 32) | out_slot_ctx.dw1 as u64,
                            ((out_slot_ctx.dw2 as u64) << 32) | out_slot_ctx.dw3 as u64,
                            0,
                        );
                        ctrl.emit_diag(
                            0x038c,
                            ((out_ep0_ctx.dw0 as u64) << 32) | out_ep0_ctx.dw1 as u64,
                            ((out_ep0_ctx.tr_dequeue_hi as u64) << 32)
                                | out_ep0_ctx.tr_dequeue_lo as u64,
                            out_ep0_ctx.dw4 as u64,
                        );
                        ctrl.emit_diag(
                            0x038d,
                            ((input_ctrl[0] as u64) << 32) | input_ctrl[1] as u64,
                            ((input_ctrl[2] as u64) << 32) | input_ctrl[3] as u64,
                            ((input_ctrl[4] as u64) << 32) | input_ctrl[5] as u64,
                        );
                        ctrl.emit_diag(
                            0x038e,
                            ((input_ctrl[6] as u64) << 32) | input_ctrl[7] as u64,
                            input_ctx.phys(host),
                            device_ctx.phys(host),
                        );
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x039b,
                            ((context_state_err_count as u64) << 32) | parameter_err_count as u64,
                            ((route as u64) << 32)
                                | ((root_hub_port as u64) << 16)
                                | ((enumerated_speed as u64) << 8)
                                | port as u64,
                            ((slot_recycles as u64) << 48)
                                | ((slot_id as u64) << 40)
                                | ((attempt_idx as u64) << 32)
                                | ((max_packet as u64) << 16)
                                | tt_encode as u64,
                        );
                        last_ctx_state_err = Some(UsbError::CmdFail(code));
                        if attempt_idx + 1 < candidate_count {
                            continue;
                        }
                    }
                    Err(UsbError::CmdFail(code)) => {
                        let tt_encode = active_tt_ctx.map(encode_address_tt_info).unwrap_or(0);
                        ctrl.emit_diag(
                            0x03b2,
                            ((slot_id as u64) << 32) | attempt_idx as u64,
                            ((slot_recycles as u64) << 48)
                                | ((max_packet as u64) << 32)
                                | tt_encode as u64,
                            encode_retry_state(
                                ADDRESS_RETRY_PATH_DIRECT_FAIL,
                                code,
                                tt_clamp_fallback_used,
                                tt_single_profile_fallback_used,
                                tt_think_time_reductions,
                                tt_parameter_retry_used,
                            ),
                        );
                        return Err(UsbError::CmdFail(code));
                    }
                    Err(err) => return Err(err),
                }
            }

            if let Some(err) = last_ctx_state_err {
                if recycle_slot_after_address_failure(
                    &ctrl,
                    &mut slot_cleanup,
                    &mut slot_id,
                    &device_ctx,
                    device_ctx_bytes,
                    &mut slot_recycles,
                    MAX_SLOT_RECYCLES,
                )? {
                    continue;
                }
                ctrl.emit_diag(
                    0x039c,
                    ((context_state_err_count as u64) << 32) | parameter_err_count as u64,
                    ((route as u64) << 32)
                        | ((root_hub_port as u64) << 16)
                        | ((enumerated_speed as u64) << 8)
                        | port as u64,
                    ((slot_recycles as u64) << 32) | slot_id as u64,
                );
                return Err(err);
            }
            return Err(UsbError::CmdFail(completion::CONTEXT_STATE_ERROR));
        }

        // Allocate ep_rings on heap to reduce stack usage
        let mut ep_rings = Vec::with_capacity(31);
        ep_rings.resize_with(31, || None);
        slot_cleanup.disarm();

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
            0x03a9,
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
                    // SAFETY: `buf` is an owned DMA allocation with at least
                    // `data_len` bytes and `d.len() <= data_len` for OUT transfers.
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
            buf.share_for_device(host, "xhci-control-buffer")?;
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
        ep0_ring.sync_for_device(host, "xhci-ep0-ring-submit")?;
        self.ctrl.emit_diag(
            0x03a4,
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
                    0x03a5,
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
                                    // SAFETY: `sample_len` is capped to both the
                                    // stack sample and DMA buffer lengths.
                                    unsafe {
                                        core::ptr::copy_nonoverlapping(
                                            buf.as_ptr::<u8>(),
                                            sample.as_mut_ptr(),
                                            sample_len,
                                        );
                                    }
                                }
                                self.ctrl.emit_diag(
                                    0x03a6,
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
                            // SAFETY: `transferred.min(d.len())` bounds the copy to
                            // the destination slice and the DMA buffer was allocated
                            // for the original setup length.
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
                    0x03aa,
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
            0x03a7,
            transferred as u64,
            u64::from_le_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
            u64::from_le_bytes([
                buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
            ]),
        );
        self.ctrl.emit_diag(
            0x03a8,
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

        // SAFETY: The descriptor request filled `buf` with exactly
        // `size_of::<DeviceDesc>()` bytes before this by-value copy.
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
                0x03a0,
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
                    0x03a1,
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
                0x03a2,
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
                    0x03a3,
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

    /// Program the slot context hub fields before enumerating downstream ports.
    ///
    /// This issues `EVALUATE_CONTEXT` with Slot Context updates so routed
    /// children can be addressed reliably behind multi-level hub topologies.
    pub fn configure_hub(&self, num_ports: u8, multi_tt: bool) -> Result<()> {
        if num_ports == 0 {
            return Err(UsbError::InvPort);
        }

        let host = self.ctrl.host();
        let ctx_stride = self.ctx_stride();
        let input_ctx_bytes = INPUT_CONTEXT_ENTRIES
            .checked_mul(ctx_stride)
            .ok_or(UsbError::OoRam)?;
        // SAFETY: `input_ctx` is owned DMA memory; helper pointers address fields
        // inside that allocation and the block rebuilds a clean Slot-only update.
        unsafe {
            // Rebuild a clean input context containing only Slot Context updates.
            core::ptr::write_bytes(self.input_ctx.as_ptr::<u8>(), 0, input_ctx_bytes);
            *self.input_drop_flags_ptr() = 0;
            *self.input_add_flags_ptr() = 1; // Slot Context

            let current_slot_ctx = *self.output_slot_ctx_ptr();
            let updated_slot_ctx =
                slot_ctx_with_hub_info(current_slot_ctx, self.speed, num_ports, multi_tt);
            *(self.input_ctx.as_ptr::<u8>().add(ctx_stride) as *mut SlotContext) = updated_slot_ctx;
        }
        compiler_fence(Ordering::Release);

        let trb = Trb {
            param: self.input_ctx.phys(host),
            status: 0,
            control: (trb_type::EVALUATE_CONTEXT << 10) | ((self.slot_id as u32) << 24),
        };
        self.ctrl.submit_command(trb)?;
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

        // Update input context.
        // SAFETY: The input-context allocation belongs to this device and is large
        // enough for the DCI being configured; endpoint descriptors are validated
        // by the caller before programming the xHCI context fields below.
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
        buf.share_for_device(host, "xhci-transfer-buffer")?;
        let trb = Trb {
            param: buf.phys(host),
            status: len as u32,
            control: (trb_type::NORMAL << 10) | (1 << 5), // IOC
        };
        ring.enqueue_and_sync(host, trb, "xhci-transfer-ring-submit")?;
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

        // Free EP0 ring. Drop cannot allocate a fallible replacement, so move
        // the real ring out and leave a non-owning sentinel behind.
        let ep0_ring = core::mem::replace(
            &mut *self.ep0_ring.lock(),
            Ring::empty_for_drop_replacement(),
        );
        ep0_ring.free(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_tt_info_sets_slot_port_and_think_time() {
        let info = encode_tt_info(TtContext {
            hub_slot_id: 0x34,
            downstream_port: 0x12,
            tt_think_time: 0x03,
            multi_tt: true,
        });
        assert_eq!(info & 0x00ff, 0x0034);
        assert_eq!((info >> TT_PORT_SHIFT) & 0x00ff, 0x0012);
        assert_eq!((info >> TT_THINK_TIME_SHIFT) & 0x0003, 0x0003);
    }

    #[test]
    fn encode_tt_info_preserves_low_ttt_values() {
        for ttt in 0u8..=2 {
            let info = encode_tt_info(TtContext {
                hub_slot_id: 1,
                downstream_port: 1,
                tt_think_time: ttt,
                multi_tt: false,
            });
            assert_eq!((info >> TT_THINK_TIME_SHIFT) & 0x0003, ttt as u32);
        }
    }

    #[test]
    fn encode_address_tt_info_omits_ttt_bits() {
        let info = encode_address_tt_info(TtContext {
            hub_slot_id: 2,
            downstream_port: 4,
            tt_think_time: 3,
            multi_tt: false,
        });
        assert_eq!(info & 0xff, 2);
        assert_eq!((info >> TT_PORT_SHIFT) & 0xff, 4);
        assert_eq!((info >> TT_THINK_TIME_SHIFT) & 0x3, 0);
    }

    #[test]
    fn clamp_tt_context_caps_ttt_to_two() {
        let tt = TtContext {
            hub_slot_id: 1,
            downstream_port: 2,
            tt_think_time: 3,
            multi_tt: true,
        };
        let clamped = clamp_tt_context(tt);
        assert_eq!(clamped.hub_slot_id, 1);
        assert_eq!(clamped.downstream_port, 2);
        assert_eq!(clamped.tt_think_time, 2);
        assert!(clamped.multi_tt);
    }

    #[test]
    fn single_tt_profile_forces_single_translator_encoding() {
        let tt = TtContext {
            hub_slot_id: 2,
            downstream_port: 4,
            tt_think_time: 3,
            multi_tt: true,
        };
        let single = single_tt_profile(tt);
        assert_eq!(single.hub_slot_id, 2);
        assert_eq!(single.downstream_port, 4);
        assert_eq!(single.tt_think_time, 2);
        assert!(!single.multi_tt);
    }

    #[test]
    fn canonicalize_tt_context_preserves_single_tt_port_and_clears_ttt() {
        let tt = TtContext {
            hub_slot_id: 7,
            downstream_port: 4,
            tt_think_time: 3,
            multi_tt: false,
        };
        let normalized = canonicalize_tt_context(tt);
        assert_eq!(normalized.hub_slot_id, 7);
        assert_eq!(normalized.downstream_port, 4);
        assert_eq!(normalized.tt_think_time, 0);
        assert!(!normalized.multi_tt);
    }

    #[test]
    fn canonicalize_tt_context_preserves_multi_tt_port_and_clears_ttt() {
        let tt = TtContext {
            hub_slot_id: 5,
            downstream_port: 6,
            tt_think_time: 3,
            multi_tt: true,
        };
        let normalized = canonicalize_tt_context(tt);
        assert_eq!(normalized.hub_slot_id, 5);
        assert_eq!(normalized.downstream_port, 6);
        assert_eq!(normalized.tt_think_time, 0);
        assert!(normalized.multi_tt);
    }

    #[test]
    fn reduced_tt_think_time_profile_decrements_until_zero() {
        let tt = TtContext {
            hub_slot_id: 2,
            downstream_port: 4,
            tt_think_time: 2,
            multi_tt: false,
        };
        let reduced_once = reduced_tt_think_time_profile(tt).expect("ttt=2 should reduce");
        assert_eq!(reduced_once.tt_think_time, 1);
        let reduced_twice =
            reduced_tt_think_time_profile(reduced_once).expect("ttt=1 should reduce");
        assert_eq!(reduced_twice.tt_think_time, 0);
        assert!(reduced_tt_think_time_profile(reduced_twice).is_none());
    }

    #[test]
    fn should_retry_with_clamped_tt_only_for_ttt_three_parameter_error() {
        let tt = Some(TtContext {
            hub_slot_id: 1,
            downstream_port: 1,
            tt_think_time: 3,
            multi_tt: false,
        });
        assert!(should_retry_with_clamped_tt(
            completion::PARAMETER_ERROR,
            tt,
            false
        ));
        assert!(!should_retry_with_clamped_tt(
            completion::PARAMETER_ERROR,
            tt,
            true
        ));
        assert!(!should_retry_with_clamped_tt(
            completion::USB_TRANSACTION_ERROR,
            tt,
            false
        ));
        assert!(!should_retry_with_clamped_tt(
            completion::PARAMETER_ERROR,
            None,
            false
        ));
    }

    #[test]
    fn should_retry_with_single_tt_profile_on_parameter_error_for_mtt_only() {
        let mtt = Some(TtContext {
            hub_slot_id: 3,
            downstream_port: 4,
            tt_think_time: 2,
            multi_tt: true,
        });
        assert!(should_retry_with_single_tt_profile(
            completion::PARAMETER_ERROR,
            mtt,
            false
        ));
        assert!(!should_retry_with_single_tt_profile(
            completion::PARAMETER_ERROR,
            mtt,
            true
        ));
        assert!(!should_retry_with_single_tt_profile(
            completion::USB_TRANSACTION_ERROR,
            mtt,
            false
        ));
    }

    #[test]
    fn should_retry_with_reduced_tt_think_time_within_budget_only() {
        let tt = Some(TtContext {
            hub_slot_id: 3,
            downstream_port: 4,
            tt_think_time: 2,
            multi_tt: false,
        });
        assert!(should_retry_with_reduced_tt_think_time(
            completion::PARAMETER_ERROR,
            tt,
            0
        ));
        assert!(should_retry_with_reduced_tt_think_time(
            completion::PARAMETER_ERROR,
            tt,
            1
        ));
        assert!(!should_retry_with_reduced_tt_think_time(
            completion::PARAMETER_ERROR,
            tt,
            2
        ));
        let zero_tt = Some(TtContext {
            hub_slot_id: 3,
            downstream_port: 4,
            tt_think_time: 0,
            multi_tt: false,
        });
        assert!(!should_retry_with_reduced_tt_think_time(
            completion::PARAMETER_ERROR,
            zero_tt,
            0
        ));
    }

    #[test]
    fn should_retry_without_tt_context_only_once_on_parameter_error() {
        let tt = Some(TtContext {
            hub_slot_id: 1,
            downstream_port: 1,
            tt_think_time: 1,
            multi_tt: false,
        });
        assert!(should_retry_without_tt_context(
            completion::PARAMETER_ERROR,
            tt,
            false
        ));
        assert!(!should_retry_without_tt_context(
            completion::PARAMETER_ERROR,
            tt,
            true
        ));
        assert!(!should_retry_without_tt_context(
            completion::PARAMETER_ERROR,
            None,
            false
        ));
        assert!(!should_retry_without_tt_context(
            completion::USB_TRANSACTION_ERROR,
            tt,
            false
        ));
    }

    #[test]
    fn should_retry_by_dropping_tt_context_only_after_exhausted_parameter_retry() {
        let tt = Some(TtContext {
            hub_slot_id: 1,
            downstream_port: 2,
            tt_think_time: 0,
            multi_tt: false,
        });
        assert!(should_retry_by_dropping_tt_context(
            completion::PARAMETER_ERROR,
            tt,
            2,
            false
        ));
        assert!(!should_retry_by_dropping_tt_context(
            completion::PARAMETER_ERROR,
            tt,
            1,
            false
        ));
        assert!(!should_retry_by_dropping_tt_context(
            completion::USB_TRANSACTION_ERROR,
            tt,
            0,
            false
        ));
        assert!(!should_retry_by_dropping_tt_context(
            completion::USB_TRANSACTION_ERROR,
            None,
            0,
            false
        ));
    }

    #[test]
    fn encode_retry_state_packs_path_code_and_flags() {
        let encoded = encode_retry_state(3, completion::PARAMETER_ERROR, true, false, 2, true);
        assert_eq!((encoded >> 56) & 0xff, 3);
        assert_eq!((encoded >> 48) & 0xff, completion::PARAMETER_ERROR as u64);
        assert_eq!((encoded >> 40) & 0xff, 1);
        assert_eq!((encoded >> 32) & 0xff, 0);
        assert_eq!((encoded >> 16) & 0xffff, 2);
        assert_eq!(encoded & 0xffff, 1);
    }

    #[test]
    fn slot_ctx_with_hub_info_sets_hub_bit_and_port_count() {
        let base = SlotContext::new(0x12345, reg::SPEED_FULL, 1, 3);
        let updated = slot_ctx_with_hub_info(base, reg::SPEED_FULL, 7, false);
        assert_ne!(updated.dw0 & SLOT_DEV_HUB, 0);
        assert_eq!((updated.dw1 >> SLOT_NUM_PORTS_SHIFT) & 0xff, 7);
        assert_eq!(updated.dw1 & 0x00ff_ffff, base.dw1 & 0x00ff_ffff);
    }

    #[test]
    fn slot_ctx_with_hub_info_sets_mtt_only_for_high_speed_hubs() {
        let base = SlotContext::new(0, reg::SPEED_HIGH, 1, 1);
        let hs_mtt = slot_ctx_with_hub_info(base, reg::SPEED_HIGH, 4, true);
        assert_ne!(hs_mtt.dw0 & SLOT_DEV_MTT, 0);

        let fs = slot_ctx_with_hub_info(base, reg::SPEED_FULL, 4, true);
        assert_eq!(fs.dw0 & SLOT_DEV_MTT, 0);

        let hs_stt = slot_ctx_with_hub_info(base, reg::SPEED_HIGH, 4, false);
        assert_eq!(hs_stt.dw0 & SLOT_DEV_MTT, 0);
    }

    #[test]
    fn ep0_hardware_recovery_is_disabled_for_control_failures() {
        assert!(!should_attempt_ep0_hardware_recovery(
            completion::STALL_ERROR,
            2
        ));
        assert!(!should_attempt_ep0_hardware_recovery(
            completion::STALL_ERROR,
            3
        ));
        assert!(!should_attempt_ep0_hardware_recovery(
            completion::USB_TRANSACTION_ERROR,
            2
        ));
        assert!(!should_attempt_ep0_hardware_recovery(0xff, 0));
    }
}
