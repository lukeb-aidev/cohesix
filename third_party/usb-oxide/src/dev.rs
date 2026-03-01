// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! USB device abstraction and context structures.

use crate::{
    Dma, Result, UsbError,
    desc::{ConfigDesc, DeviceDesc, EndpointDesc, SetupPacket, desc_type},
    reg,
    ring::{PhysMem, Ring, Trb, completion, trb_type},
    xhci::XhciCtrl,
};

use alloc::{sync::Arc, vec::Vec};
use core::hint::spin_loop;
use core::sync::atomic::{Ordering, compiler_fence};
use spin::Mutex;

const CONTROL_XFER_WAIT_SPINS: usize = 20_000_000;
const ROUTE_STRING_MASK: u32 = 0x000f_ffff;
const ROOT_HUB_PORT_SHIFT: u32 = 16;
const SLOT_DEV_MTT: u32 = 1 << 25;
const TT_PORT_SHIFT: u32 = 8;
const CONTEXT_ALIGN_BYTES: usize = 64;
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

        // Enable slot
        let mut slot_id = ctrl.enable_slot()?;

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
        let mut slot_ctx_template = SlotContext::new(route, speed, 1, root_hub_port);
        if let Some(tt) = tt_context {
            if tt.multi_tt {
                slot_ctx_template.dw0 |= SLOT_DEV_MTT;
            }
            slot_ctx_template.dw2 =
                (tt.hub_slot_id as u32) | ((tt.downstream_port as u32) << TT_PORT_SHIFT);
        }

        // Set device context in DCBAA
        unsafe {
            core::ptr::write_bytes(device_ctx.as_ptr::<u8>(), 0, device_ctx_bytes);
            core::ptr::write_bytes(input_base, 0, input_ctx_bytes);
        }
        ctrl.set_device_context(slot_id, device_ctx.phys(host));
        compiler_fence(Ordering::Release);

        // Build EP0 max-packet candidates. Some controllers/devices are strict
        // about the initial EP0 MPS used during Address Device.
        let mut ep0_mps_candidates = [0u16; 3];
        let mut candidate_count = 0usize;
        let mut push_candidate = |mps: u16| {
            if !ep0_mps_candidates[..candidate_count].contains(&mps) && candidate_count < 3 {
                ep0_mps_candidates[candidate_count] = mps;
                candidate_count += 1;
            }
        };
        match speed {
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
                    ((attempt_idx as u64) << 32) | speed as u64,
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
        mut data: Option<&mut [u8]>,
    ) -> Result<usize> {
        let host = self.ctrl.host();
        let mut ep0_ring = self.ep0_ring.lock();

        let data_dir = (setup.request_type & 0x80) != 0; // true = IN
        let data_len = data.as_ref().map(|d| d.len()).unwrap_or(0);

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

        // Setup Stage TRB
        let setup_trb = Trb {
            param: unsafe { *(setup as *const SetupPacket as *const u64) },
            status: 8, // Transfer length = 8
            control: (trb_type::SETUP << 10)
                | (1 << 6) // IDT (Immediate Data)
                | if data_len > 0 && setup.length > 0 {
                    if data_dir { 3 << 16 } else { 2 << 16 } // TRT: IN or OUT
                } else {
                    0 // No data stage
                },
        };
        ep0_ring.enqueue(host, setup_trb);

        // Data Stage TRB (if needed)
        if let Some(ref buf) = data_buf {
            let data_trb = Trb {
                param: buf.phys(host),
                status: setup.length as u32,
                control: (trb_type::DATA << 10)
                    | if data_dir { 1 << 16 } else { 0 } // DIR
                    | (1 << 5), // IOC for debugging
            };
            ep0_ring.enqueue(host, data_trb);
        }

        // Status Stage TRB
        let status_trb = Trb {
            param: 0,
            status: 0,
            control: (trb_type::STATUS << 10)
                | if data_len > 0 && setup.length > 0 && data_dir { 0 } else { 1 << 16 } // DIR
                | (1 << 5), // IOC
        };
        ep0_ring.enqueue(host, status_trb);

        drop(ep0_ring);

        // Ring doorbell for EP0 (target = 1)
        self.ctrl.ring_doorbell(self.slot_id, 1);

        // Wait for completion
        let mut waited = 0usize;
        loop {
            if let Some(evt) = self.ctrl.poll_event()
                && evt.trb_type() == trb_type::TRANSFER_EVENT as u8
                && evt.slot_id() == self.slot_id
            {
                let code = evt.completion_code();
                match code {
                    completion::SUCCESS | completion::SHORT_PACKET => {
                        let transferred =
                            (setup.length as usize).saturating_sub(evt.transfer_length() as usize);

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
                        if let Some(buf) = data_buf {
                            buf.free(host);
                        }
                        return Err(UsbError::Stall);
                    }
                    _ => {
                        if let Some(buf) = data_buf {
                            buf.free(host);
                        }
                        return Err(UsbError::XferFail(code));
                    }
                }
            }
            waited = waited.saturating_add(1);
            if waited >= CONTROL_XFER_WAIT_SPINS {
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
        self.control_transfer(&setup, Some(&mut buf))?;

        let desc = unsafe { *(buf.as_ptr() as *const DeviceDesc) };
        self.device_desc = Some(desc);
        Ok(desc)
    }

    /// Get configuration descriptor (full, with interfaces and endpoints)
    pub fn get_config_descriptor(&self, index: u8) -> Result<Vec<u8>> {
        // First, get just the config descriptor to find total length
        let mut buf = [0u8; 9];
        let setup = SetupPacket::get_descriptor(desc_type::CONFIGURATION, index, 9);
        self.control_transfer(&setup, Some(&mut buf))?;

        let config = unsafe { *(buf.as_ptr() as *const ConfigDesc) };
        let total_len = config.total_length as usize;

        // Now get the full descriptor
        let mut full_buf = alloc::vec![0u8; total_len];
        let setup = SetupPacket::get_descriptor(desc_type::CONFIGURATION, index, total_len as u16);
        self.control_transfer(&setup, Some(&mut full_buf))?;

        Ok(full_buf)
    }

    /// Set configuration
    pub fn set_configuration(&self, config: u8) -> Result<()> {
        let setup = SetupPacket::set_configuration(config);
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
        let ep0_ring = core::mem::replace(&mut *self.ep0_ring.lock(), Ring::new(host, 1).unwrap());
        ep0_ring.free(host);
    }
}
