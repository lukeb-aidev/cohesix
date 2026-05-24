// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide fixed-ring runtime support for isolated Pi 4 driver images.
// Author: Lukas Bower

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![allow(unsafe_code)]

use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU32, Ordering},
};

use font8x8::legacy::BASIC_LEGACY;
use pi4_driver_abi::{
    DriverRuntimeInitDescriptor, DRIVER_RUNTIME_ENGINE_INIT_AUX,
    DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_RGB888, DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
    DRIVER_RUNTIME_INIT_AUX, DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX, DRIVER_RUNTIME_NET_INIT_AUX,
    DRIVER_RUNTIME_PCIE_OP_PORT_READ, DRIVER_RUNTIME_PCIE_OP_PORT_WRITE,
    DRIVER_RUNTIME_PCIE_OP_POSTED_WRITE_FLUSH, DRIVER_RUNTIME_SDIO_FLAG_DATA,
    DRIVER_RUNTIME_SDIO_FLAG_RESP_LONG, DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE,
    DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR, DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
    DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY, DRIVER_RUNTIME_SDIO_FLAG_WRITE, HOT_PATH_CYW43_WIFI,
    HOT_PATH_GENET_NIC, HOT_PATH_HDMI_TEXT, HOT_PATH_PCIE_ROOT, HOT_PATH_SDIO_HOST,
    HOT_PATH_SERIAL_CONSOLE, HOT_PATH_USB_KEYBOARD,
};

/// Child CSpace slot containing the root-to-driver command endpoint.
pub const DRIVER_TASK_CHILD_COMMAND_SLOT: sel4_sys::seL4_CPtr = 2;
/// Driver-local fixed virtual address for the command/completion ring.
pub const DRIVER_TASK_RING_VADDR: usize = 0x7000_0000;
/// First fixed driver-local virtual address reserved for explicit MMIO pages.
pub const DRIVER_TASK_DEVICE_MMIO_VADDR: usize = 0x7000_4000;
/// Offset of the completion record in the command/completion ring page.
pub const DRIVER_TASK_RING_COMPLETION_OFFSET: usize = 64;
/// Offset of the shared payload area in the command/completion ring page.
pub const DRIVER_TASK_RING_FRAME_OFFSET: usize = 256;
/// Bytes in the command/completion ring page.
pub const DRIVER_TASK_RING_PAGE_BYTES: usize = 4096;
/// Maximum frame admitted by the root-task ABI.
pub const MAX_DRIVER_TASK_FRAME_BYTES: usize = 1536;

const OPCODE_SERVICE: u16 = 1;
const OPCODE_SUBMIT_FRAME: u16 = 3;
const OPCODE_SHUTDOWN: u16 = 5;

const COMPLETION_PROGRESS: u16 = 1;
const COMPLETION_FRAME_READY: u16 = 2;
const COMPLETION_IDLE: u16 = 3;
const COMPLETION_FAULT: u16 = 5;

const FAULT_NONE: u16 = 0;
const FAULT_REJECTED_COMMAND: u16 = 1;
const FAULT_DEVICE_UNAVAILABLE: u16 = 3;

const SERIAL_RUNTIME_AUX_INIT: u32 = 0x5345_5249;

const ROLE_SERIAL: u32 = 1 << 0;
const ROLE_USB: u32 = 1 << 1;
const ROLE_DISPLAY: u32 = 1 << 2;
const ROLE_NET: u32 = 1 << 3;
const ROLE_SDIO: u32 = 1 << 4;
const ROLE_PCIE: u32 = 1 << 5;

const ENGINE_STATE_INITIALIZED: u32 = 1 << 0;
const ENGINE_STATE_DESCRIPTOR_READY: u32 = 1 << 1;
const ENGINE_STATE_RESOURCE_READY: u32 = 1 << 2;
const ENGINE_STATE_TX_PROGRESS: u32 = 1 << 3;
const ENGINE_STATE_RX_PROGRESS: u32 = 1 << 4;

const CHAR_WIDTH: usize = 8;
const CHAR_HEIGHT: usize = 16;
const FB_BYTES_PER_PIXEL_32: usize = 4;
const HDMI_FG_COLOR: u32 = 0xffff_ffff;
const HDMI_BG_COLOR: u32 = 0xff00_0000;
const USB_BOOT_REPORT_BYTES: usize = 8;
const USB_KEYBOARD_OUTPUT_LIMIT: usize = 16;

const SDHCI_BLOCK_SIZE: usize = 0x04;
const SDHCI_BLOCK_COUNT: usize = 0x06;
const SDHCI_ARGUMENT: usize = 0x08;
const SDHCI_TRANSFER_MODE: usize = 0x0c;
const SDHCI_COMMAND: usize = 0x0e;
const SDHCI_RESPONSE: usize = 0x10;
const SDHCI_BUFFER: usize = 0x20;
const SDHCI_PRESENT_STATE: usize = 0x24;
const SDHCI_INT_STATUS: usize = 0x30;
const SDHCI_TRNS_BLK_CNT_EN: u16 = 1 << 1;
const SDHCI_TRNS_READ: u16 = 1 << 4;
const SDHCI_CMD_RESP_NONE: u16 = 0x00;
const SDHCI_CMD_RESP_LONG: u16 = 0x01;
const SDHCI_CMD_RESP_SHORT: u16 = 0x02;
const SDHCI_CMD_RESP_SHORT_BUSY: u16 = 0x03;
const SDHCI_CMD_CRC: u16 = 0x08;
const SDHCI_CMD_INDEX: u16 = 0x10;
const SDHCI_CMD_DATA: u16 = 0x20;
const SDHCI_CMD_INHIBIT: u32 = 1 << 0;
const SDHCI_DATA_INHIBIT: u32 = 1 << 1;
const SDHCI_SPACE_AVAILABLE: u32 = 1 << 10;
const SDHCI_DATA_AVAILABLE: u32 = 1 << 11;
const SDHCI_INT_RESPONSE: u32 = 1 << 0;
const SDHCI_INT_DATA_END: u32 = 1 << 1;
const SDHCI_INT_SPACE_AVAIL: u32 = 1 << 4;
const SDHCI_INT_DATA_AVAIL: u32 = 1 << 5;
const SDHCI_INT_ERROR: u32 = 1 << 15;
const SDHCI_INT_TIMEOUT: u32 = 1 << 16;
const SDHCI_INT_CRC: u32 = 1 << 17;
const SDHCI_INT_END_BIT: u32 = 1 << 18;
const SDHCI_INT_INDEX: u32 = 1 << 19;
const SDHCI_INT_DATA_TIMEOUT: u32 = 1 << 20;
const SDHCI_INT_DATA_CRC: u32 = 1 << 21;
const SDHCI_INT_DATA_END_BIT: u32 = 1 << 22;
const SDHCI_INT_COMMAND_DATA_CLEAR_MASK: u32 = u32::MAX;
const SDHCI_CMD_WAIT_LOOPS: usize = 100_000;

const USB_REQUIRED_MMIO_PAGES: u16 = 2;
const USB_REQUIRED_DMA_PAGES: u16 = 16;
const USB_REQUIRED_SHARED_PAGES: u16 = 2;
const HDMI_REQUIRED_MMIO_PAGES: u16 = 1;
const HDMI_REQUIRED_DMA_PAGES: u16 = 1;
const HDMI_REQUIRED_SHARED_PAGES: u16 = 2;
const GENET_REQUIRED_MMIO_PAGES: u16 = 6;
const GENET_REQUIRED_DMA_PAGES: u16 = 64;
const GENET_REQUIRED_SHARED_PAGES: u16 = 4;
const CYW43_REQUIRED_DMA_PAGES: u16 = 8;
const CYW43_REQUIRED_SHARED_PAGES: u16 = 4;
const SDIO_REQUIRED_MMIO_PAGES: u16 = 1;
const SDIO_REQUIRED_DMA_PAGES: u16 = 2;
const SDIO_REQUIRED_SHARED_PAGES: u16 = 2;
const PCIE_REQUIRED_MMIO_PAGES: u16 = 10;
const PCIE_REQUIRED_SHARED_PAGES: u16 = 1;

struct RuntimeDescriptorSlot {
    descriptor: UnsafeCell<DriverRuntimeInitDescriptor>,
}

// SAFETY: Each isolated runtime image is single-TCB by construction. Root
// submits one synchronous ring command at a time, and the runtime copies the
// descriptor before publishing completion.
unsafe impl Sync for RuntimeDescriptorSlot {}

impl RuntimeDescriptorSlot {
    const fn new() -> Self {
        Self {
            descriptor: UnsafeCell::new(DriverRuntimeInitDescriptor::empty()),
        }
    }

    fn store(&self, descriptor: DriverRuntimeInitDescriptor) {
        // SAFETY: See the `Sync` invariant above; there is exactly one runtime
        // TCB mutating this cell and root does not map this static.
        unsafe {
            core::ptr::write_volatile(self.descriptor.get(), descriptor);
        }
    }

    fn load(&self) -> DriverRuntimeInitDescriptor {
        // SAFETY: See the `Sync` invariant above; volatile keeps command-turn
        // state visible to tests and target code without inventing references.
        unsafe { core::ptr::read_volatile(self.descriptor.get()) }
    }
}

static RUNTIME_DESCRIPTOR: RuntimeDescriptorSlot = RuntimeDescriptorSlot::new();
static RUNTIME_INIT_HOT_PATH: AtomicU32 = AtomicU32::new(0);
static RUNTIME_INIT_FLAGS: AtomicU32 = AtomicU32::new(0);
static USB_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static HDMI_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static HDMI_CURSOR_ROW: AtomicU32 = AtomicU32::new(0);
static HDMI_CURSOR_COL: AtomicU32 = AtomicU32::new(0);
static GENET_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static GENET_TX_COUNT: AtomicU32 = AtomicU32::new(0);
static GENET_RX_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_COUNT: AtomicU32 = AtomicU32::new(0);
static SDIO_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static SDIO_CMD_COUNT: AtomicU32 = AtomicU32::new(0);
static PCIE_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static PCIE_OP_COUNT: AtomicU32 = AtomicU32::new(0);

#[cfg(target_os = "none")]
const MINI_UART_IO_OFFSET: usize = 0x40;
#[cfg(target_os = "none")]
const MINI_UART_IER_OFFSET: usize = 0x44;
#[cfg(target_os = "none")]
const MINI_UART_IIR_OFFSET: usize = 0x48;
#[cfg(target_os = "none")]
const MINI_UART_LCR_OFFSET: usize = 0x4c;
#[cfg(target_os = "none")]
const MINI_UART_MCR_OFFSET: usize = 0x50;
#[cfg(target_os = "none")]
const MINI_UART_LSR_OFFSET: usize = 0x54;
#[cfg(target_os = "none")]
const MINI_UART_CNTL_OFFSET: usize = 0x60;
#[cfg(target_os = "none")]
const AUX_ENABLES_OFFSET: usize = 0x04;
#[cfg(target_os = "none")]
const MINI_UART_LSR_RX_READY: u32 = 1;
#[cfg(target_os = "none")]
const MINI_UART_LSR_TX_EMPTY: u32 = 1 << 5;
#[cfg(target_os = "none")]
const MINI_UART_TX_SPIN_LIMIT: usize = 1024;
const MINI_UART_RX_DRAIN_LIMIT: usize = 128;

/// Shared-buffer descriptor passed over the pointer-free driver-task ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverFrameDescriptor {
    /// Offset into the ring/shared-buffer arena.
    pub offset: u32,
    /// Valid payload length at `offset`.
    pub len: u16,
    /// Role-specific primitive flags.
    pub flags: u16,
}

impl DriverFrameDescriptor {
    const fn empty() -> Self {
        Self {
            offset: 0,
            len: 0,
            flags: 0,
        }
    }

    fn in_ring_payload(self) -> bool {
        let offset = self.offset as usize;
        let len = self.len as usize;
        len <= MAX_DRIVER_TASK_FRAME_BYTES
            && offset >= DRIVER_TASK_RING_FRAME_OFFSET
            && offset
                .checked_add(len)
                .is_some_and(|end| end <= DRIVER_TASK_RING_PAGE_BYTES)
    }
}

/// Primitive budget grant encoded in the root/driver ring ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskBudgetGrant {
    /// Maximum HAL operations admitted for the command.
    pub max_ops: u16,
    /// Maximum frames admitted for the command.
    pub max_frames: u16,
    /// Maximum bytes admitted for the command.
    pub max_bytes: u32,
}

/// Fixed command record consumed by isolated driver runtimes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskCommandRecord {
    /// Monotonic root-assigned sequence number.
    pub sequence: u32,
    /// Primitive command opcode.
    pub opcode: u16,
    /// Role-specific primitive flags.
    pub flags: u16,
    /// Opcode-specific primitive argument.
    pub arg0: u32,
    /// Second opcode-specific primitive argument.
    pub arg1: u32,
    /// Auxiliary role-specific argument.
    pub aux0: u32,
    /// Second auxiliary role-specific argument.
    pub aux1: u32,
    /// Per-command service budget.
    pub budget: DriverTaskBudgetGrant,
    /// Shared-buffer descriptor for frame-bearing commands.
    pub frame: DriverFrameDescriptor,
}

/// Completion record written by isolated driver runtimes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskCompletionRecord {
    /// Completed command sequence.
    pub sequence: u32,
    /// Primitive completion code.
    pub code: u16,
    /// Fault code or role-specific detail.
    pub detail: u16,
    /// Role-specific primitive result.
    pub result: u32,
    /// Shared-buffer descriptor for frame-bearing completions.
    pub frame: DriverFrameDescriptor,
}

impl DriverTaskCompletionRecord {
    const fn progress(sequence: u32, result: u32) -> Self {
        Self {
            sequence,
            code: COMPLETION_PROGRESS,
            detail: FAULT_NONE,
            result,
            frame: DriverFrameDescriptor::empty(),
        }
    }

    const fn frame_ready(sequence: u32, len: u16) -> Self {
        Self {
            sequence,
            code: COMPLETION_FRAME_READY,
            detail: FAULT_NONE,
            result: len as u32,
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len,
                flags: 0,
            },
        }
    }

    const fn idle(sequence: u32) -> Self {
        Self {
            sequence,
            code: COMPLETION_IDLE,
            detail: FAULT_NONE,
            result: 0,
            frame: DriverFrameDescriptor::empty(),
        }
    }

    const fn fault(sequence: u32, detail: u16) -> Self {
        Self {
            sequence,
            code: COMPLETION_FAULT,
            detail,
            result: 0,
            frame: DriverFrameDescriptor::empty(),
        }
    }
}

/// Service one fixed-layout command without using root pointers.
#[must_use]
pub fn service_command(
    task_key: usize,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord {
    if command.sequence == 0 {
        return DriverTaskCompletionRecord::idle(0);
    }
    if command.opcode == OPCODE_SHUTDOWN {
        return DriverTaskCompletionRecord::progress(command.sequence, 1);
    }
    if command.opcode == OPCODE_SERVICE && command.arg0 == 0 {
        return DriverTaskCompletionRecord::progress(command.sequence, task_key as u32);
    }
    let Some(role) = role_for_hot_path(command.arg0) else {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    };
    if command.arg1 != role {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    if command.aux0 == DRIVER_RUNTIME_INIT_AUX {
        return service_runtime_init(command);
    }
    if RUNTIME_INIT_HOT_PATH.load(Ordering::Acquire) != command.arg0 {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !opcode_matches_hot_path(command.opcode, command.arg0) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }

    match command.arg0 {
        HOT_PATH_SERIAL_CONSOLE => service_serial(command),
        HOT_PATH_USB_KEYBOARD => service_usb_keyboard(command),
        HOT_PATH_HDMI_TEXT => service_hdmi_text(command),
        HOT_PATH_GENET_NIC => service_net_engine(
            command,
            &GENET_RUNTIME_FLAGS,
            &GENET_TX_COUNT,
            &GENET_RX_COUNT,
        ),
        HOT_PATH_CYW43_WIFI => service_net_engine(
            command,
            &CYW43_RUNTIME_FLAGS,
            &CYW43_TX_COUNT,
            &CYW43_RX_COUNT,
        ),
        HOT_PATH_SDIO_HOST => service_sdio_host(command),
        HOT_PATH_PCIE_ROOT => service_pcie_root(command),
        _ => DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND),
    }
}

#[must_use]
fn validate_runtime_init_descriptor(
    command: DriverTaskCommandRecord,
    descriptor: DriverRuntimeInitDescriptor,
) -> DriverTaskCompletionRecord {
    if command.opcode != OPCODE_SERVICE
        || command.frame.len as usize != core::mem::size_of::<DriverRuntimeInitDescriptor>()
    {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    if !descriptor.valid()
        || descriptor.hot_path != command.arg0
        || descriptor.role_bit != command.arg1
    {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    RUNTIME_DESCRIPTOR.store(descriptor);
    RUNTIME_INIT_HOT_PATH.store(descriptor.hot_path, Ordering::Release);
    RUNTIME_INIT_FLAGS.store(descriptor.flags, Ordering::Release);
    mark_descriptor_ready(descriptor.hot_path);
    DriverTaskCompletionRecord::progress(command.sequence, descriptor.hot_path)
}

#[cfg(target_os = "none")]
fn service_runtime_init(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if !command.frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let descriptor_addr = DRIVER_TASK_RING_VADDR + command.frame.offset as usize;
    // SAFETY: The frame descriptor is bounds-checked against the fixed ring
    // page. Root stages `DriverRuntimeInitDescriptor` at an aligned offset in
    // the ring payload before submitting the init command.
    let descriptor =
        unsafe { core::ptr::read_volatile(descriptor_addr as *const DriverRuntimeInitDescriptor) };
    validate_runtime_init_descriptor(command, descriptor)
}

#[cfg(not(target_os = "none"))]
fn service_runtime_init(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE)
}

/// Host-test helper for exercising runtime init without mapping the fixed ring.
#[cfg(test)]
#[must_use]
fn service_runtime_init_for_test(
    command: DriverTaskCommandRecord,
    descriptor: DriverRuntimeInitDescriptor,
) -> DriverTaskCompletionRecord {
    validate_runtime_init_descriptor(command, descriptor)
}

fn role_for_hot_path(hot_path: u32) -> Option<u32> {
    match hot_path {
        HOT_PATH_SERIAL_CONSOLE => Some(ROLE_SERIAL),
        HOT_PATH_USB_KEYBOARD => Some(ROLE_USB),
        HOT_PATH_HDMI_TEXT => Some(ROLE_DISPLAY),
        HOT_PATH_GENET_NIC | HOT_PATH_CYW43_WIFI => Some(ROLE_NET),
        HOT_PATH_SDIO_HOST => Some(ROLE_SDIO),
        HOT_PATH_PCIE_ROOT => Some(ROLE_PCIE),
        _ => None,
    }
}

fn opcode_matches_hot_path(opcode: u16, hot_path: u32) -> bool {
    if hot_path == HOT_PATH_HDMI_TEXT {
        opcode == OPCODE_SUBMIT_FRAME
    } else {
        opcode == OPCODE_SERVICE
    }
}

fn runtime_flags_for_hot_path(hot_path: u32) -> Option<&'static AtomicU32> {
    match hot_path {
        HOT_PATH_USB_KEYBOARD => Some(&USB_RUNTIME_FLAGS),
        HOT_PATH_HDMI_TEXT => Some(&HDMI_RUNTIME_FLAGS),
        HOT_PATH_GENET_NIC => Some(&GENET_RUNTIME_FLAGS),
        HOT_PATH_CYW43_WIFI => Some(&CYW43_RUNTIME_FLAGS),
        HOT_PATH_SDIO_HOST => Some(&SDIO_RUNTIME_FLAGS),
        HOT_PATH_PCIE_ROOT => Some(&PCIE_RUNTIME_FLAGS),
        _ => None,
    }
}

fn mark_descriptor_ready(hot_path: u32) {
    if let Some(flags) = runtime_flags_for_hot_path(hot_path) {
        flags.fetch_or(ENGINE_STATE_DESCRIPTOR_READY, Ordering::AcqRel);
    }
}

fn mark_engine_initialized(hot_path: u32) -> bool {
    let descriptor = RUNTIME_DESCRIPTOR.load();
    if descriptor.hot_path != hot_path || RUNTIME_INIT_HOT_PATH.load(Ordering::Acquire) != hot_path
    {
        return false;
    }
    let Some(flags) = runtime_flags_for_hot_path(hot_path) else {
        return false;
    };
    let resource_ready = descriptor_resources_ready(descriptor, hot_path);
    if !resource_ready {
        return false;
    }
    let mut bits = ENGINE_STATE_INITIALIZED | ENGINE_STATE_DESCRIPTOR_READY;
    bits |= ENGINE_STATE_RESOURCE_READY;
    flags.fetch_or(bits, Ordering::AcqRel);
    true
}

fn descriptor_resources_ready(descriptor: DriverRuntimeInitDescriptor, hot_path: u32) -> bool {
    match hot_path {
        HOT_PATH_USB_KEYBOARD => {
            descriptor.mmio_page_count >= USB_REQUIRED_MMIO_PAGES
                && descriptor.dma_page_count >= USB_REQUIRED_DMA_PAGES
                && descriptor.shared_page_count >= USB_REQUIRED_SHARED_PAGES
        }
        HOT_PATH_HDMI_TEXT => {
            descriptor.mmio_page_count >= HDMI_REQUIRED_MMIO_PAGES
                && descriptor.dma_page_count >= HDMI_REQUIRED_DMA_PAGES
                && descriptor.shared_page_count >= HDMI_REQUIRED_SHARED_PAGES
                && descriptor.hdmi_ready()
        }
        HOT_PATH_GENET_NIC => {
            descriptor.mmio_page_count >= GENET_REQUIRED_MMIO_PAGES
                && descriptor.dma_page_count >= GENET_REQUIRED_DMA_PAGES
                && descriptor.shared_page_count >= GENET_REQUIRED_SHARED_PAGES
        }
        HOT_PATH_CYW43_WIFI => {
            descriptor.dma_page_count >= CYW43_REQUIRED_DMA_PAGES
                && descriptor.shared_page_count >= CYW43_REQUIRED_SHARED_PAGES
        }
        HOT_PATH_SDIO_HOST => {
            descriptor.mmio_page_count >= SDIO_REQUIRED_MMIO_PAGES
                && descriptor.dma_page_count >= SDIO_REQUIRED_DMA_PAGES
                && descriptor.shared_page_count >= SDIO_REQUIRED_SHARED_PAGES
        }
        HOT_PATH_PCIE_ROOT => {
            descriptor.mmio_page_count >= PCIE_REQUIRED_MMIO_PAGES
                && descriptor.shared_page_count >= PCIE_REQUIRED_SHARED_PAGES
        }
        _ => false,
    }
}

fn engine_initialized(flags: &AtomicU32) -> bool {
    flags.load(Ordering::Acquire) & ENGINE_STATE_INITIALIZED != 0
}

fn service_engine_init(command: DriverTaskCommandRecord) -> Option<DriverTaskCompletionRecord> {
    let init_aux = matches!(
        command.aux0,
        DRIVER_RUNTIME_ENGINE_INIT_AUX
            | DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX
            | DRIVER_RUNTIME_NET_INIT_AUX
    );
    if !init_aux {
        return None;
    }
    if command.frame.len != 0 {
        return Some(DriverTaskCompletionRecord::fault(
            command.sequence,
            FAULT_REJECTED_COMMAND,
        ));
    }
    if mark_engine_initialized(command.arg0) {
        Some(DriverTaskCompletionRecord::progress(command.sequence, 1))
    } else {
        Some(DriverTaskCompletionRecord::fault(
            command.sequence,
            FAULT_DEVICE_UNAVAILABLE,
        ))
    }
}

fn service_serial(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if command.aux0 == SERIAL_RUNTIME_AUX_INIT {
        serial_init_mini_uart();
        return DriverTaskCompletionRecord::progress(command.sequence, 1);
    }
    if command.frame.len == 0 {
        let limit = command
            .budget
            .max_bytes
            .min(MAX_DRIVER_TASK_FRAME_BYTES as u32)
            .min(MINI_UART_RX_DRAIN_LIMIT as u32) as usize;
        let read = serial_read_frame(limit);
        return if read == 0 {
            DriverTaskCompletionRecord::idle(command.sequence)
        } else {
            DriverTaskCompletionRecord::frame_ready(command.sequence, read as u16)
        };
    }
    if !command.frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let written = serial_write_frame(command.frame);
    if written == 0 {
        DriverTaskCompletionRecord::idle(command.sequence)
    } else {
        DriverTaskCompletionRecord::progress(command.sequence, written as u32)
    }
}

fn service_usb_keyboard(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !engine_initialized(&USB_RUNTIME_FLAGS) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    if command.frame.len == 0 {
        return DriverTaskCompletionRecord::idle(command.sequence);
    }
    if !command.frame.in_ring_payload() || command.frame.len as usize != USB_BOOT_REPORT_BYTES {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let produced = usb_keyboard_report_to_frame(command.frame);
    if produced == 0 {
        DriverTaskCompletionRecord::idle(command.sequence)
    } else {
        USB_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_RX_PROGRESS, Ordering::AcqRel);
        DriverTaskCompletionRecord::frame_ready(command.sequence, produced as u16)
    }
}

fn service_hdmi_text(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if command.frame.len == 0 {
        return DriverTaskCompletionRecord::idle(command.sequence);
    }
    if !command.frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    if RUNTIME_INIT_HOT_PATH.load(Ordering::Acquire) != HOT_PATH_HDMI_TEXT {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    if !RUNTIME_DESCRIPTOR.load().hdmi_ready() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    let rendered = hdmi_render_frame(command.frame);
    if rendered == 0 {
        DriverTaskCompletionRecord::idle(command.sequence)
    } else {
        HDMI_RUNTIME_FLAGS.fetch_or(
            ENGINE_STATE_INITIALIZED
                | ENGINE_STATE_DESCRIPTOR_READY
                | ENGINE_STATE_RESOURCE_READY
                | ENGINE_STATE_TX_PROGRESS,
            Ordering::AcqRel,
        );
        DriverTaskCompletionRecord::progress(command.sequence, rendered as u32)
    }
}

fn service_net_engine(
    command: DriverTaskCommandRecord,
    flags: &AtomicU32,
    tx_count: &AtomicU32,
    _rx_count: &AtomicU32,
) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !engine_initialized(flags) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    if command.frame.len == 0 {
        return DriverTaskCompletionRecord::idle(command.sequence);
    }
    if !command.frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    flags.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
    tx_count.fetch_add(1, Ordering::AcqRel);
    DriverTaskCompletionRecord::progress(command.sequence, command.frame.len as u32)
}

fn service_sdio_host(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !engine_initialized(&SDIO_RUNTIME_FLAGS) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    if command.frame.len != 0 && !command.frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let command_index = (command.aux0 >> 16) & 0x3f;
    let flags = (command.aux0 & 0xffff) as u16;
    let data_len = command.frame.len as u32;
    if command_index > 63 {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    if data_len != 0 && flags & DRIVER_RUNTIME_SDIO_FLAG_DATA == 0 {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    if flags & DRIVER_RUNTIME_SDIO_FLAG_WRITE != 0 && data_len == 0 {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    if sdio_response_flag_count(flags) != 1 {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let Some(response0) =
        sdio_execute_command(command_index as u16, command.aux1, flags, command.frame)
    else {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    };
    SDIO_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
    SDIO_CMD_COUNT.fetch_add(1, Ordering::AcqRel);
    if data_len != 0 && flags & DRIVER_RUNTIME_SDIO_FLAG_WRITE == 0 {
        DriverTaskCompletionRecord::frame_ready(command.sequence, command.frame.len)
    } else if data_len != 0 {
        DriverTaskCompletionRecord::progress(command.sequence, data_len)
    } else {
        DriverTaskCompletionRecord::progress(command.sequence, response0)
    }
}

fn sdio_response_flag_count(flags: u16) -> u32 {
    let response_flags = flags
        & (DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE
            | DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR
            | DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT
            | DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY
            | DRIVER_RUNTIME_SDIO_FLAG_RESP_LONG);
    response_flags.count_ones()
}

#[cfg(target_os = "none")]
fn sdio_execute_command(
    cmd: u16,
    arg: u32,
    flags: u16,
    frame: DriverFrameDescriptor,
) -> Option<u32> {
    let has_data = flags & DRIVER_RUNTIME_SDIO_FLAG_DATA != 0;
    let write = flags & DRIVER_RUNTIME_SDIO_FLAG_WRITE != 0;
    if !sdio_wait_inhibit_clear(has_data) {
        return None;
    }
    sdio_write32(SDHCI_INT_STATUS, SDHCI_INT_COMMAND_DATA_CLEAR_MASK);
    if has_data {
        sdio_write16(SDHCI_BLOCK_SIZE, frame.len);
        sdio_write16(SDHCI_BLOCK_COUNT, 1);
    } else {
        sdio_write16(SDHCI_TRANSFER_MODE, 0);
    }
    sdio_write32(SDHCI_ARGUMENT, arg);
    if has_data {
        let mut transfer = SDHCI_TRNS_BLK_CNT_EN;
        if !write {
            transfer |= SDHCI_TRNS_READ;
        }
        sdio_write16(SDHCI_TRANSFER_MODE, transfer);
    }
    sdio_write16(SDHCI_COMMAND, sdio_make_command(cmd, flags, has_data));
    let cmd_status = sdio_wait_int(SDHCI_INT_RESPONSE | SDHCI_INT_ERROR);
    if cmd_status & SDHCI_INT_ERROR != 0 || cmd_status == 0 {
        return None;
    }
    if has_data && !sdio_transfer_frame(frame, write) {
        return None;
    }
    if flags & DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE != 0 {
        Some(0)
    } else {
        Some(sdio_read32(SDHCI_RESPONSE))
    }
}

#[cfg(not(target_os = "none"))]
fn sdio_execute_command(
    _cmd: u16,
    _arg: u32,
    _flags: u16,
    _frame: DriverFrameDescriptor,
) -> Option<u32> {
    Some(0)
}

#[cfg(target_os = "none")]
fn sdio_make_command(cmd: u16, flags: u16, data: bool) -> u16 {
    let mut command = if flags & DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE != 0 {
        SDHCI_CMD_RESP_NONE
    } else if flags & DRIVER_RUNTIME_SDIO_FLAG_RESP_LONG != 0 {
        SDHCI_CMD_RESP_LONG | SDHCI_CMD_CRC
    } else if flags & DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY != 0 {
        SDHCI_CMD_RESP_SHORT_BUSY | SDHCI_CMD_CRC | SDHCI_CMD_INDEX
    } else {
        SDHCI_CMD_RESP_SHORT | SDHCI_CMD_CRC | SDHCI_CMD_INDEX
    };
    if data {
        command |= SDHCI_CMD_DATA;
    }
    (cmd << 8) | command
}

#[cfg(target_os = "none")]
fn sdio_wait_inhibit_clear(wait_data: bool) -> bool {
    let mask = if wait_data {
        SDHCI_CMD_INHIBIT | SDHCI_DATA_INHIBIT
    } else {
        SDHCI_CMD_INHIBIT
    };
    for _ in 0..SDHCI_CMD_WAIT_LOOPS {
        if sdio_read32(SDHCI_PRESENT_STATE) & mask == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

#[cfg(target_os = "none")]
fn sdio_wait_int(mask: u32) -> u32 {
    let error_mask = SDHCI_INT_ERROR
        | SDHCI_INT_TIMEOUT
        | SDHCI_INT_CRC
        | SDHCI_INT_END_BIT
        | SDHCI_INT_INDEX
        | SDHCI_INT_DATA_TIMEOUT
        | SDHCI_INT_DATA_CRC
        | SDHCI_INT_DATA_END_BIT;
    for _ in 0..SDHCI_CMD_WAIT_LOOPS {
        let status = sdio_read32(SDHCI_INT_STATUS);
        if status & (mask | error_mask) != 0 {
            sdio_write32(SDHCI_INT_STATUS, status);
            return status;
        }
        core::hint::spin_loop();
    }
    0
}

#[cfg(target_os = "none")]
fn sdio_transfer_frame(frame: DriverFrameDescriptor, write: bool) -> bool {
    let mut offset = 0usize;
    while offset < frame.len as usize {
        let ready = if write {
            SDHCI_SPACE_AVAILABLE | SDHCI_INT_SPACE_AVAIL
        } else {
            SDHCI_DATA_AVAILABLE | SDHCI_INT_DATA_AVAIL
        };
        if sdio_read32(SDHCI_PRESENT_STATE) & ready == 0 {
            let status = sdio_wait_int(ready | SDHCI_INT_ERROR);
            if status & SDHCI_INT_ERROR != 0 || status == 0 {
                return false;
            }
        }
        let word_len = (frame.len as usize - offset).min(4);
        if write {
            let mut word = 0u32;
            for byte_index in 0..word_len {
                word |= u32::from(read_ring_byte(frame.offset as usize + offset + byte_index))
                    << (byte_index * 8);
            }
            sdio_write32(SDHCI_BUFFER, word);
        } else {
            let word = sdio_read32(SDHCI_BUFFER);
            for byte_index in 0..word_len {
                write_ring_byte(
                    frame.offset as usize + offset + byte_index,
                    ((word >> (byte_index * 8)) & 0xff) as u8,
                );
            }
        }
        offset = offset.saturating_add(word_len);
    }
    let status = sdio_wait_int(SDHCI_INT_DATA_END | SDHCI_INT_ERROR);
    status & SDHCI_INT_ERROR == 0 && status != 0
}

fn service_pcie_root(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !engine_initialized(&PCIE_RUNTIME_FLAGS) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    let op = (command.aux0 >> 16) as u16;
    let offset = command.aux1 as usize;
    let descriptor = RUNTIME_DESCRIPTOR.load();
    let mmio_limit =
        usize::from(descriptor.mmio_page_count).saturating_mul(DRIVER_TASK_RING_PAGE_BYTES);
    if offset & 0x3 != 0 || offset >= mmio_limit {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    match op {
        DRIVER_RUNTIME_PCIE_OP_PORT_READ => {
            if command.frame.len != 0 {
                return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
            }
            let value = pcie_read32(offset);
            PCIE_OP_COUNT.fetch_add(1, Ordering::AcqRel);
            PCIE_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_RX_PROGRESS, Ordering::AcqRel);
            DriverTaskCompletionRecord::progress(command.sequence, value)
        }
        DRIVER_RUNTIME_PCIE_OP_PORT_WRITE => {
            let value = match pcie_frame_write_value(command.frame) {
                Ok(value) => value,
                Err(detail) => {
                    return DriverTaskCompletionRecord::fault(command.sequence, detail);
                }
            };
            pcie_write32(offset, value);
            PCIE_OP_COUNT.fetch_add(1, Ordering::AcqRel);
            PCIE_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
            DriverTaskCompletionRecord::progress(command.sequence, 1)
        }
        DRIVER_RUNTIME_PCIE_OP_POSTED_WRITE_FLUSH => {
            if command.frame.len != 0 {
                return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
            }
            let _ = pcie_read32(offset);
            PCIE_OP_COUNT.fetch_add(1, Ordering::AcqRel);
            DriverTaskCompletionRecord::progress(command.sequence, 1)
        }
        _ => DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND),
    }
}

fn pcie_frame_write_value(frame: DriverFrameDescriptor) -> Result<u32, u16> {
    if frame.len == 0 {
        return Ok(0);
    }
    if frame.len != 4 || !frame.in_ring_payload() {
        return Err(FAULT_REJECTED_COMMAND);
    }
    Ok(read_ring_u32(frame.offset as usize))
}

fn usb_keyboard_report_to_frame(frame: DriverFrameDescriptor) -> usize {
    let report = read_frame_prefix::<USB_BOOT_REPORT_BYTES>(frame);
    let mut produced = 0usize;
    for &code in report[2..].iter() {
        if code == 0 {
            continue;
        }
        let Some(byte) = usb_hid_usage_to_ascii(code, report[0] & 0x22 != 0) else {
            continue;
        };
        write_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + produced, byte);
        produced = produced.saturating_add(1);
        if produced >= USB_KEYBOARD_OUTPUT_LIMIT {
            break;
        }
    }
    produced
}

fn usb_hid_usage_to_ascii(code: u8, shifted: bool) -> Option<u8> {
    match code {
        0x04..=0x1d => {
            let base = if shifted { b'A' } else { b'a' };
            Some(base + (code - 0x04))
        }
        0x1e..=0x26 => Some(if shifted {
            b"!@#$%^&*("[usize::from(code - 0x1e)]
        } else {
            b"123456789"[usize::from(code - 0x1e)]
        }),
        0x27 => Some(if shifted { b')' } else { b'0' }),
        0x28 => Some(b'\n'),
        0x2a => Some(0x08),
        0x2c => Some(b' '),
        0x2d => Some(if shifted { b'_' } else { b'-' }),
        0x2e => Some(if shifted { b'+' } else { b'=' }),
        0x2f => Some(if shifted { b'{' } else { b'[' }),
        0x30 => Some(if shifted { b'}' } else { b']' }),
        0x31 => Some(if shifted { b'|' } else { b'\\' }),
        0x33 => Some(if shifted { b':' } else { b';' }),
        0x34 => Some(if shifted { b'"' } else { b'\'' }),
        0x35 => Some(if shifted { b'~' } else { b'`' }),
        0x36 => Some(if shifted { b'<' } else { b',' }),
        0x37 => Some(if shifted { b'>' } else { b'.' }),
        0x38 => Some(if shifted { b'?' } else { b'/' }),
        _ => None,
    }
}

fn hdmi_render_frame(frame: DriverFrameDescriptor) -> usize {
    let descriptor = RUNTIME_DESCRIPTOR.load();
    let mut state = HdmiRenderState::from_descriptor(descriptor);
    let mut rendered = 0usize;
    for index in 0..frame.len as usize {
        let byte = read_ring_byte(frame.offset as usize + index);
        state.put_byte(byte);
        rendered = rendered.saturating_add(1);
    }
    HDMI_CURSOR_ROW.store(state.row as u32, Ordering::Release);
    HDMI_CURSOR_COL.store(state.col as u32, Ordering::Release);
    rendered
}

struct HdmiRenderState {
    framebuffer: usize,
    framebuffer_len: usize,
    width: usize,
    height: usize,
    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    text_height: usize,
    pitch: usize,
    format: u32,
    cols: usize,
    rows: usize,
    row: usize,
    col: usize,
}

impl HdmiRenderState {
    fn from_descriptor(descriptor: DriverRuntimeInitDescriptor) -> Self {
        let width = descriptor.framebuffer.width as usize;
        let height = descriptor.framebuffer.height as usize;
        let pitch = descriptor.framebuffer.pitch as usize;
        let rows = (height / CHAR_HEIGHT).max(1);
        let text_height = rows.saturating_mul(CHAR_HEIGHT).min(height);
        let framebuffer_len = pitch.saturating_mul(height);
        Self {
            framebuffer: descriptor.framebuffer.vaddr as usize,
            framebuffer_len,
            width,
            height,
            text_height,
            pitch,
            format: descriptor.framebuffer.format,
            cols: (width / CHAR_WIDTH).max(1),
            rows,
            row: HDMI_CURSOR_ROW.load(Ordering::Acquire) as usize,
            col: HDMI_CURSOR_COL.load(Ordering::Acquire) as usize,
        }
    }

    fn put_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            0x08 | 0x7f => self.backspace(),
            b'\t' => {
                for _ in 0..4 {
                    self.put_byte(b' ');
                }
            }
            byte => {
                if self.col >= self.cols {
                    self.newline();
                }
                self.draw_char(byte);
                self.col = self.col.saturating_add(1);
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row = self.row.saturating_add(1);
        if self.row >= self.rows {
            self.scroll_up_one_text_row();
            self.row = self.rows.saturating_sub(1);
        }
    }

    fn backspace(&mut self) {
        if self.col == 0 {
            if self.row != 0 {
                self.row = self.row.saturating_sub(1);
                self.col = self.cols.saturating_sub(1);
            }
        } else {
            self.col = self.col.saturating_sub(1);
        }
        self.draw_char(b' ');
    }

    fn scroll_up_one_text_row(&mut self) {
        #[cfg(target_os = "none")]
        {
            let scroll_pixels = CHAR_HEIGHT.min(self.text_height);
            let visible_row_bytes = self.width.saturating_mul(FB_BYTES_PER_PIXEL_32);
            let scroll_bytes = self.pitch.saturating_mul(scroll_pixels);
            let total_bytes = self.pitch.saturating_mul(self.text_height);
            if scroll_pixels == 0
                || total_bytes == 0
                || total_bytes > self.framebuffer_len
                || scroll_bytes >= total_bytes
                || visible_row_bytes == 0
                || visible_row_bytes > self.pitch
            {
                self.clear_screen();
                return;
            }
            let move_bytes = total_bytes.saturating_sub(scroll_bytes);
            // SAFETY: The runtime init descriptor bounds the mapped framebuffer
            // and the copy stays inside the text viewport.
            unsafe {
                core::ptr::copy(
                    (self.framebuffer + scroll_bytes) as *const u8,
                    self.framebuffer as *mut u8,
                    move_bytes,
                );
            }
            self.fill_rect(
                0,
                self.text_height.saturating_sub(scroll_pixels),
                self.width,
                scroll_pixels,
                HDMI_BG_COLOR,
            );
        }
    }

    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    fn clear_screen(&mut self) {
        self.fill_rect(0, 0, self.width, self.height, HDMI_BG_COLOR);
    }

    fn draw_char(&mut self, byte: u8) {
        let glyph = BASIC_LEGACY[usize::from(byte.min(0x7f))];
        let x0 = self.col.saturating_mul(CHAR_WIDTH);
        let y0 = self.row.saturating_mul(CHAR_HEIGHT);
        self.fill_rect(x0, y0, CHAR_WIDTH, CHAR_HEIGHT, HDMI_BG_COLOR);
        for (gy, bits) in glyph.iter().enumerate() {
            for gx in 0..CHAR_WIDTH {
                if ((bits >> gx) & 1) == 0 {
                    continue;
                }
                let x = x0.saturating_add(gx);
                let y = y0.saturating_add(gy.saturating_mul(2));
                self.put_pixel(x, y, HDMI_FG_COLOR);
                self.put_pixel(x, y.saturating_add(1), HDMI_FG_COLOR);
            }
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = self.width.min(x.saturating_add(w));
        let y_end = self.height.min(y.saturating_add(h));
        if x >= x_end || y >= y_end {
            return;
        }
        for yy in y..y_end {
            for xx in x..x_end {
                self.put_pixel(xx, yy, color);
            }
        }
    }

    fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let Some(row_off) = y.checked_mul(self.pitch) else {
            return;
        };
        let bytes_per_pixel = match self.format {
            DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_RGB888 => 3,
            DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888 | 0 => FB_BYTES_PER_PIXEL_32,
            _ => return,
        };
        let Some(col_off) = x.checked_mul(bytes_per_pixel) else {
            return;
        };
        let Some(byte_off) = row_off.checked_add(col_off) else {
            return;
        };
        let Some(end) = byte_off.checked_add(bytes_per_pixel) else {
            return;
        };
        if end > self.framebuffer_len {
            return;
        }
        write_framebuffer_pixel(self.framebuffer + byte_off, color, bytes_per_pixel);
    }
}

fn read_frame_prefix<const N: usize>(frame: DriverFrameDescriptor) -> [u8; N] {
    let mut out = [0u8; N];
    let len = (frame.len as usize).min(N);
    for (index, slot) in out.iter_mut().enumerate().take(len) {
        *slot = read_ring_byte(frame.offset as usize + index);
    }
    out
}

#[cfg(target_os = "none")]
fn read_ring_byte(offset: usize) -> u8 {
    // SAFETY: Callers validate frame descriptors before reading payload bytes.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_RING_VADDR + offset) as *const u8) }
}

#[cfg(not(target_os = "none"))]
fn read_ring_byte(_offset: usize) -> u8 {
    0
}

#[cfg(target_os = "none")]
fn write_ring_byte(offset: usize, value: u8) {
    // SAFETY: Callers write only into the fixed shared ring payload region.
    unsafe {
        core::ptr::write_volatile((DRIVER_TASK_RING_VADDR + offset) as *mut u8, value);
    }
}

#[cfg(not(target_os = "none"))]
fn write_ring_byte(_offset: usize, _value: u8) {}

#[cfg(target_os = "none")]
fn read_ring_u32(offset: usize) -> u32 {
    let b0 = u32::from(read_ring_byte(offset));
    let b1 = u32::from(read_ring_byte(offset + 1));
    let b2 = u32::from(read_ring_byte(offset + 2));
    let b3 = u32::from(read_ring_byte(offset + 3));
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

#[cfg(not(target_os = "none"))]
fn read_ring_u32(_offset: usize) -> u32 {
    0
}

#[cfg(target_os = "none")]
fn write_framebuffer_pixel(addr: usize, color: u32, bytes_per_pixel: usize) {
    match bytes_per_pixel {
        3 => {
            // SAFETY: The caller bounds-checks the framebuffer byte range.
            unsafe {
                core::ptr::write_volatile(addr as *mut u8, (color & 0xff) as u8);
                core::ptr::write_volatile((addr + 1) as *mut u8, ((color >> 8) & 0xff) as u8);
                core::ptr::write_volatile((addr + 2) as *mut u8, ((color >> 16) & 0xff) as u8);
            }
        }
        4 => {
            // SAFETY: The caller bounds-checks the framebuffer byte range and
            // the mapped framebuffer is writable device memory.
            unsafe {
                core::ptr::write_volatile(addr as *mut u32, color);
            }
        }
        _ => {}
    }
}

#[cfg(not(target_os = "none"))]
fn write_framebuffer_pixel(_addr: usize, _color: u32, _bytes_per_pixel: usize) {}

#[cfg(target_os = "none")]
fn pcie_read32(offset: usize) -> u32 {
    // SAFETY: `service_pcie_root` validates offset alignment and bounds against
    // the mapped PCIe/VL805 MMIO aperture before this read.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u32) }
}

#[cfg(not(target_os = "none"))]
fn pcie_read32(_offset: usize) -> u32 {
    0
}

#[cfg(target_os = "none")]
fn pcie_write32(offset: usize, value: u32) {
    // SAFETY: `service_pcie_root` validates offset alignment and bounds against
    // the mapped PCIe/VL805 MMIO aperture before this write.
    unsafe {
        core::ptr::write_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *mut u32, value);
    }
}

#[cfg(not(target_os = "none"))]
fn pcie_write32(_offset: usize, _value: u32) {}

#[cfg(target_os = "none")]
fn sdio_read32(offset: usize) -> u32 {
    // SAFETY: The SDIO runtime maps the declared SDHCI MMIO page at
    // `DRIVER_TASK_DEVICE_MMIO_VADDR`; all offsets used by callers are bounded
    // register constants within that page.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u32) }
}

#[cfg(target_os = "none")]
fn sdio_write32(offset: usize, value: u32) {
    // SAFETY: The SDIO runtime maps the declared SDHCI MMIO page at
    // `DRIVER_TASK_DEVICE_MMIO_VADDR`; all offsets used by callers are bounded
    // register constants within that page.
    unsafe {
        core::ptr::write_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *mut u32, value);
    }
}

#[cfg(target_os = "none")]
fn sdio_write16(offset: usize, value: u16) {
    // SAFETY: The SDIO runtime maps the declared SDHCI MMIO page at
    // `DRIVER_TASK_DEVICE_MMIO_VADDR`; all offsets used by callers are bounded
    // register constants within that page and accept 16-bit accesses.
    unsafe {
        core::ptr::write_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *mut u16, value);
    }
}

#[cfg(target_os = "none")]
fn serial_init_mini_uart() {
    // SAFETY: The serial runtime image maps the declared BCM2711 auxiliary UART
    // MMIO page at `DRIVER_TASK_DEVICE_MMIO_VADDR`. All offsets below are
    // within that page and mirror the root-task mini-UART driver's conservative
    // polled setup.
    unsafe {
        let enables = core::ptr::read_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + AUX_ENABLES_OFFSET) as *const u32,
        );
        core::ptr::write_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + AUX_ENABLES_OFFSET) as *mut u32,
            enables | 0x1,
        );
        core::ptr::write_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_IER_OFFSET) as *mut u32,
            0,
        );
        core::ptr::write_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_CNTL_OFFSET) as *mut u32,
            0,
        );
        core::ptr::write_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_LCR_OFFSET) as *mut u32,
            0x3,
        );
        core::ptr::write_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_MCR_OFFSET) as *mut u32,
            0,
        );
        core::ptr::write_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_IIR_OFFSET) as *mut u32,
            0xc6,
        );
        core::ptr::write_volatile(
            (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_CNTL_OFFSET) as *mut u32,
            0x3,
        );
    }
}

#[cfg(not(target_os = "none"))]
fn serial_init_mini_uart() {}

#[cfg(target_os = "none")]
fn serial_read_frame(limit: usize) -> usize {
    let mut read = 0usize;
    let limit = limit.min(MAX_DRIVER_TASK_FRAME_BYTES);
    while read < limit {
        // SAFETY: The serial runtime image maps exactly the declared UART MMIO
        // page at `DRIVER_TASK_DEVICE_MMIO_VADDR`; the MU_LSR offset is within
        // that page and is read-only for this polling check.
        let lsr = unsafe {
            core::ptr::read_volatile(
                (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_LSR_OFFSET) as *const u32,
            )
        };
        if lsr & MINI_UART_LSR_RX_READY == 0 {
            break;
        }
        // SAFETY: MU_IO returns one received byte when MU_LSR reports data
        // ready; the write target is inside the fixed shared ring payload.
        let byte = unsafe {
            core::ptr::read_volatile(
                (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_IO_OFFSET) as *const u32,
            ) as u8
        };
        // SAFETY: `read < limit <= MAX_DRIVER_TASK_FRAME_BYTES`, so this write
        // stays within the fixed ring payload region.
        unsafe {
            core::ptr::write_volatile(
                (DRIVER_TASK_RING_VADDR + DRIVER_TASK_RING_FRAME_OFFSET + read) as *mut u8,
                byte,
            );
        }
        read = read.saturating_add(1);
    }
    read
}

#[cfg(not(target_os = "none"))]
fn serial_read_frame(_limit: usize) -> usize {
    0
}

#[cfg(target_os = "none")]
fn serial_write_frame(frame: DriverFrameDescriptor) -> usize {
    let mut written = 0usize;
    let src = (DRIVER_TASK_RING_VADDR + frame.offset as usize) as *const u8;
    let uart = DRIVER_TASK_DEVICE_MMIO_VADDR as *mut u32;
    for index in 0..frame.len as usize {
        // SAFETY: The frame descriptor is validated by `service_serial`, the
        // ring page is mapped at `DRIVER_TASK_RING_VADDR`, and byte reads stay
        // within the fixed page-local payload region.
        let byte = unsafe { core::ptr::read_volatile(src.add(index)) };
        if !wait_for_mini_uart_tx(uart) {
            break;
        }
        // SAFETY: The serial runtime image maps exactly the declared UART MMIO
        // page at `DRIVER_TASK_DEVICE_MMIO_VADDR`; the MU_IO offset is within
        // that page and accepts 32-bit writes of one transmit byte.
        unsafe {
            core::ptr::write_volatile(
                (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_IO_OFFSET) as *mut u32,
                u32::from(byte),
            );
        }
        written = written.saturating_add(1);
    }
    written
}

#[cfg(target_os = "none")]
fn wait_for_mini_uart_tx(_uart: *mut u32) -> bool {
    for _ in 0..MINI_UART_TX_SPIN_LIMIT {
        // SAFETY: The serial runtime image maps the UART MMIO page at
        // `DRIVER_TASK_DEVICE_MMIO_VADDR`; the MU_LSR offset is in the same
        // page and is read-only for this polling check.
        let lsr = unsafe {
            core::ptr::read_volatile(
                (DRIVER_TASK_DEVICE_MMIO_VADDR + MINI_UART_LSR_OFFSET) as *const u32,
            )
        };
        if lsr & MINI_UART_LSR_TX_EMPTY != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

#[cfg(not(target_os = "none"))]
fn serial_write_frame(frame: DriverFrameDescriptor) -> usize {
    frame.len as usize
}

/// Run the isolated driver runtime receive/service loop.
#[cfg(target_os = "none")]
pub fn runtime_main(task_key: usize) -> ! {
    loop {
        let mut badge: sel4_sys::seL4_Word = 0;
        // SAFETY: The root task minted `DRIVER_TASK_CHILD_COMMAND_SLOT` into
        // the child CSpace before resuming this TCB. The runtime only waits on
        // that endpoint and consumes primitive command records from the mapped
        // ring page.
        unsafe {
            let _ = sel4_sys::seL4_Recv(DRIVER_TASK_CHILD_COMMAND_SLOT, &mut badge);
        }
        let _ = badge;
        // SAFETY: Root maps one command/completion ring page at the fixed
        // driver-local address before starting the runtime.
        let command = unsafe {
            core::ptr::read_volatile(DRIVER_TASK_RING_VADDR as *const DriverTaskCommandRecord)
        };
        let completion = service_command(task_key, command);
        // SAFETY: The completion record lies in the same mapped ring page and
        // uses the fixed ABI layout shared with root.
        unsafe {
            core::ptr::write_volatile(
                (DRIVER_TASK_RING_VADDR + DRIVER_TASK_RING_COMPLETION_OFFSET)
                    as *mut DriverTaskCompletionRecord,
                completion,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi4_driver_abi::{
        DriverRuntimeFramebufferDescriptor, DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
        DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER,
    };

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock()
            .expect("runtime tests must serialize global state")
    }

    fn reset_runtime_for_test() {
        RUNTIME_DESCRIPTOR.store(DriverRuntimeInitDescriptor::empty());
        RUNTIME_INIT_HOT_PATH.store(0, Ordering::Release);
        RUNTIME_INIT_FLAGS.store(0, Ordering::Release);
        USB_RUNTIME_FLAGS.store(0, Ordering::Release);
        HDMI_RUNTIME_FLAGS.store(0, Ordering::Release);
        HDMI_CURSOR_ROW.store(0, Ordering::Release);
        HDMI_CURSOR_COL.store(0, Ordering::Release);
        GENET_RUNTIME_FLAGS.store(0, Ordering::Release);
        GENET_TX_COUNT.store(0, Ordering::Release);
        GENET_RX_COUNT.store(0, Ordering::Release);
        CYW43_RUNTIME_FLAGS.store(0, Ordering::Release);
        CYW43_TX_COUNT.store(0, Ordering::Release);
        CYW43_RX_COUNT.store(0, Ordering::Release);
        SDIO_RUNTIME_FLAGS.store(0, Ordering::Release);
        SDIO_CMD_COUNT.store(0, Ordering::Release);
        PCIE_RUNTIME_FLAGS.store(0, Ordering::Release);
        PCIE_OP_COUNT.store(0, Ordering::Release);
    }

    fn budget() -> DriverTaskBudgetGrant {
        DriverTaskBudgetGrant {
            max_ops: 1,
            max_frames: 1,
            max_bytes: 64,
        }
    }

    fn descriptor_for(hot_path: u32, role: u32) -> DriverRuntimeInitDescriptor {
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
        descriptor.hot_path = hot_path;
        descriptor.role_bit = role;
        descriptor.flags = pi4_driver_abi::DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            | pi4_driver_abi::DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        let (mmio_pages, dma_pages, shared_pages) = match hot_path {
            HOT_PATH_USB_KEYBOARD => (
                USB_REQUIRED_MMIO_PAGES,
                USB_REQUIRED_DMA_PAGES,
                USB_REQUIRED_SHARED_PAGES,
            ),
            HOT_PATH_HDMI_TEXT => (
                HDMI_REQUIRED_MMIO_PAGES,
                HDMI_REQUIRED_DMA_PAGES,
                HDMI_REQUIRED_SHARED_PAGES,
            ),
            HOT_PATH_GENET_NIC => (
                GENET_REQUIRED_MMIO_PAGES,
                GENET_REQUIRED_DMA_PAGES,
                GENET_REQUIRED_SHARED_PAGES,
            ),
            HOT_PATH_CYW43_WIFI => (0, CYW43_REQUIRED_DMA_PAGES, CYW43_REQUIRED_SHARED_PAGES),
            HOT_PATH_SDIO_HOST => (
                SDIO_REQUIRED_MMIO_PAGES,
                SDIO_REQUIRED_DMA_PAGES,
                SDIO_REQUIRED_SHARED_PAGES,
            ),
            HOT_PATH_PCIE_ROOT => (PCIE_REQUIRED_MMIO_PAGES, 0, PCIE_REQUIRED_SHARED_PAGES),
            _ => (1, 0, 1),
        };
        descriptor.mmio_page_count = mmio_pages;
        descriptor.dma_page_count = dma_pages;
        descriptor.shared_page_count = shared_pages;
        for index in 0..usize::from(mmio_pages) {
            descriptor.mmio_pages[index] =
                pi4_driver_abi::DriverRuntimePageDescriptor::new(0x1000_0000 + index * 0x1000);
        }
        for index in 0..usize::from(dma_pages) {
            descriptor.dma_pages[index] =
                pi4_driver_abi::DriverRuntimePageDescriptor::new(0x2000_0000 + index * 0x1000);
        }
        for index in 0..usize::from(shared_pages) {
            descriptor.shared_pages[index] =
                pi4_driver_abi::DriverRuntimePageDescriptor::new(0x4000_0000 + index * 0x1000);
        }
        descriptor
    }

    fn init_runtime_for_test(hot_path: u32, role: u32) {
        let command = DriverTaskCommandRecord {
            sequence: 1,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: hot_path,
            arg1: role,
            aux0: DRIVER_RUNTIME_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: core::mem::size_of::<DriverRuntimeInitDescriptor>() as u16,
                flags: 0,
            },
        };
        assert_eq!(
            service_runtime_init_for_test(command, descriptor_for(hot_path, role)),
            DriverTaskCompletionRecord::progress(1, hot_path)
        );
    }

    #[test]
    fn wire_records_match_root_task_layout_sizes() {
        let _guard = test_guard();
        reset_runtime_for_test();
        assert_eq!(core::mem::size_of::<DriverFrameDescriptor>(), 8);
        assert_eq!(core::mem::align_of::<DriverFrameDescriptor>(), 4);
        assert_eq!(core::mem::size_of::<DriverTaskBudgetGrant>(), 8);
        assert_eq!(core::mem::align_of::<DriverTaskBudgetGrant>(), 4);
        assert_eq!(core::mem::size_of::<DriverTaskCommandRecord>(), 40);
        assert_eq!(core::mem::align_of::<DriverTaskCommandRecord>(), 4);
        assert_eq!(core::mem::size_of::<DriverTaskCompletionRecord>(), 20);
        assert_eq!(core::mem::align_of::<DriverTaskCompletionRecord>(), 4);
    }

    #[test]
    fn smoke_service_returns_task_key_without_root_context() {
        let _guard = test_guard();
        reset_runtime_for_test();
        let command = DriverTaskCommandRecord {
            sequence: 9,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: 0,
            arg1: 0,
            aux0: 0,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(4, command),
            DriverTaskCompletionRecord::progress(9, 4)
        );
    }

    #[test]
    fn serial_init_command_marks_runtime_attached() {
        let _guard = test_guard();
        reset_runtime_for_test();
        init_runtime_for_test(HOT_PATH_SERIAL_CONSOLE, ROLE_SERIAL);
        let command = DriverTaskCommandRecord {
            sequence: 13,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_SERIAL_CONSOLE,
            arg1: ROLE_SERIAL,
            aux0: SERIAL_RUNTIME_AUX_INIT,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::progress(13, 1)
        );
    }

    #[test]
    fn malformed_hot_path_command_is_rejected() {
        let _guard = test_guard();
        reset_runtime_for_test();
        let command = DriverTaskCommandRecord {
            sequence: 10,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_GENET_NIC,
            arg1: ROLE_USB,
            aux0: 0,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::fault(10, FAULT_REJECTED_COMMAND)
        );
    }

    #[test]
    fn non_serial_hardware_work_fails_closed_until_real_runtime_exists() {
        let _guard = test_guard();
        reset_runtime_for_test();
        let command = DriverTaskCommandRecord {
            sequence: 11,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_GENET_NIC,
            arg1: ROLE_NET,
            aux0: 0,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: 64,
                flags: 0,
            },
        };
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::fault(11, FAULT_DEVICE_UNAVAILABLE)
        );
    }

    #[test]
    fn runtime_init_descriptor_is_pointer_free_and_role_checked() {
        let _guard = test_guard();
        reset_runtime_for_test();
        let descriptor = descriptor_for(HOT_PATH_GENET_NIC, ROLE_NET);

        let command = DriverTaskCommandRecord {
            sequence: 14,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_GENET_NIC,
            arg1: ROLE_NET,
            aux0: DRIVER_RUNTIME_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: core::mem::size_of::<DriverRuntimeInitDescriptor>() as u16,
                flags: 0,
            },
        };

        assert_eq!(
            service_runtime_init_for_test(command, descriptor),
            DriverTaskCompletionRecord::progress(14, HOT_PATH_GENET_NIC)
        );

        let mut wrong_role = descriptor;
        wrong_role.role_bit = ROLE_USB;
        assert_eq!(
            service_runtime_init_for_test(command, wrong_role),
            DriverTaskCompletionRecord::fault(14, FAULT_REJECTED_COMMAND)
        );
    }

    #[test]
    fn hdmi_frame_is_bounded_but_not_claimed_without_framebuffer_runtime() {
        let _guard = test_guard();
        reset_runtime_for_test();
        let command = DriverTaskCommandRecord {
            sequence: 12,
            opcode: OPCODE_SUBMIT_FRAME,
            flags: 0,
            arg0: HOT_PATH_HDMI_TEXT,
            arg1: ROLE_DISPLAY,
            aux0: 0,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: 16,
                flags: 0,
            },
        };
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::fault(12, FAULT_DEVICE_UNAVAILABLE)
        );
    }

    #[test]
    fn linked_net_usb_sdio_and_pcie_handlers_are_stateful_after_engine_init() {
        let _guard = test_guard();
        reset_runtime_for_test();
        init_runtime_for_test(HOT_PATH_GENET_NIC, ROLE_NET);
        let mut init = DriverTaskCommandRecord {
            sequence: 20,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_GENET_NIC,
            arg1: ROLE_NET,
            aux0: DRIVER_RUNTIME_NET_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, init),
            DriverTaskCompletionRecord::progress(20, 1)
        );
        let tx = DriverTaskCommandRecord {
            sequence: 21,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_GENET_NIC,
            arg1: ROLE_NET,
            aux0: 0,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: 64,
                flags: 0,
            },
        };
        assert_eq!(
            service_command(0, tx),
            DriverTaskCompletionRecord::progress(21, 64)
        );

        reset_runtime_for_test();
        init_runtime_for_test(HOT_PATH_USB_KEYBOARD, ROLE_USB);
        init.arg0 = HOT_PATH_USB_KEYBOARD;
        init.arg1 = ROLE_USB;
        init.aux0 = DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX;
        assert_eq!(
            service_command(0, init),
            DriverTaskCompletionRecord::progress(20, 1)
        );
        let idle = DriverTaskCommandRecord {
            sequence: 22,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_USB_KEYBOARD,
            arg1: ROLE_USB,
            aux0: 0,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, idle),
            DriverTaskCompletionRecord::idle(22)
        );

        reset_runtime_for_test();
        init_runtime_for_test(HOT_PATH_SDIO_HOST, ROLE_SDIO);
        init.arg0 = HOT_PATH_SDIO_HOST;
        init.arg1 = ROLE_SDIO;
        init.aux0 = DRIVER_RUNTIME_ENGINE_INIT_AUX;
        assert_eq!(
            service_command(0, init),
            DriverTaskCompletionRecord::progress(20, 1)
        );
        let mut sdio = idle;
        sdio.sequence = 23;
        sdio.arg0 = HOT_PATH_SDIO_HOST;
        sdio.arg1 = ROLE_SDIO;
        sdio.aux0 = (52 << 16) | u32::from(DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT);
        assert_eq!(
            service_command(0, sdio),
            DriverTaskCompletionRecord::progress(23, 0)
        );

        reset_runtime_for_test();
        init_runtime_for_test(HOT_PATH_PCIE_ROOT, ROLE_PCIE);
        init.arg0 = HOT_PATH_PCIE_ROOT;
        init.arg1 = ROLE_PCIE;
        init.aux0 = DRIVER_RUNTIME_ENGINE_INIT_AUX;
        assert_eq!(
            service_command(0, init),
            DriverTaskCompletionRecord::progress(20, 1)
        );
        let mut pcie = idle;
        pcie.sequence = 24;
        pcie.arg0 = HOT_PATH_PCIE_ROOT;
        pcie.arg1 = ROLE_PCIE;
        pcie.aux0 = (DRIVER_RUNTIME_PCIE_OP_POSTED_WRITE_FLUSH as u32) << 16;
        pcie.aux1 = 0x100;
        assert_eq!(
            service_command(0, pcie),
            DriverTaskCompletionRecord::progress(24, 1)
        );
    }

    #[test]
    fn engine_init_rejects_descriptors_without_required_resources() {
        let _guard = test_guard();
        reset_runtime_for_test();
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
        descriptor.hot_path = HOT_PATH_GENET_NIC;
        descriptor.role_bit = ROLE_NET;
        descriptor.flags = pi4_driver_abi::DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            | pi4_driver_abi::DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = pi4_driver_abi::DriverRuntimePageDescriptor::new(0x4000_0000);
        let init_descriptor = DriverTaskCommandRecord {
            sequence: 40,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_GENET_NIC,
            arg1: ROLE_NET,
            aux0: DRIVER_RUNTIME_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: core::mem::size_of::<DriverRuntimeInitDescriptor>() as u16,
                flags: 0,
            },
        };
        assert_eq!(
            service_runtime_init_for_test(init_descriptor, descriptor),
            DriverTaskCompletionRecord::progress(40, HOT_PATH_GENET_NIC)
        );
        let engine_init = DriverTaskCommandRecord {
            sequence: 41,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_GENET_NIC,
            arg1: ROLE_NET,
            aux0: DRIVER_RUNTIME_NET_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, engine_init),
            DriverTaskCompletionRecord::fault(41, FAULT_DEVICE_UNAVAILABLE)
        );
    }

    #[test]
    fn sdio_runtime_requires_data_flag_and_one_response_class() {
        let _guard = test_guard();
        reset_runtime_for_test();
        init_runtime_for_test(HOT_PATH_SDIO_HOST, ROLE_SDIO);
        let init = DriverTaskCommandRecord {
            sequence: 50,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_SDIO_HOST,
            arg1: ROLE_SDIO,
            aux0: DRIVER_RUNTIME_ENGINE_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, init),
            DriverTaskCompletionRecord::progress(50, 1)
        );

        let mut command = DriverTaskCommandRecord {
            sequence: 51,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_SDIO_HOST,
            arg1: ROLE_SDIO,
            aux0: (53 << 16) | u32::from(DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT),
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: 4,
                flags: 0,
            },
        };
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::fault(51, FAULT_REJECTED_COMMAND)
        );
        command.sequence = 52;
        command.aux0 = (53 << 16)
            | u32::from(DRIVER_RUNTIME_SDIO_FLAG_DATA)
            | u32::from(DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT)
            | u32::from(DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR);
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::fault(52, FAULT_REJECTED_COMMAND)
        );
        command.sequence = 53;
        command.frame = DriverFrameDescriptor::empty();
        command.aux0 = u32::from(DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE);
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::progress(53, 0)
        );
        command.sequence = 54;
        command.frame = DriverFrameDescriptor {
            offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
            len: 8,
            flags: 0,
        };
        command.aux0 = (53 << 16)
            | u32::from(DRIVER_RUNTIME_SDIO_FLAG_DATA)
            | u32::from(DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT);
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::frame_ready(54, 8)
        );
    }

    #[test]
    fn pcie_runtime_bounds_mmio_by_descriptor_pages() {
        let _guard = test_guard();
        reset_runtime_for_test();
        init_runtime_for_test(HOT_PATH_PCIE_ROOT, ROLE_PCIE);
        let init = DriverTaskCommandRecord {
            sequence: 60,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_PCIE_ROOT,
            arg1: ROLE_PCIE,
            aux0: DRIVER_RUNTIME_ENGINE_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, init),
            DriverTaskCompletionRecord::progress(60, 1)
        );
        let limit = usize::from(PCIE_REQUIRED_MMIO_PAGES) * DRIVER_TASK_RING_PAGE_BYTES;
        let command = DriverTaskCommandRecord {
            sequence: 61,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_PCIE_ROOT,
            arg1: ROLE_PCIE,
            aux0: (DRIVER_RUNTIME_PCIE_OP_PORT_READ as u32) << 16,
            aux1: limit as u32,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, command),
            DriverTaskCompletionRecord::fault(61, FAULT_REJECTED_COMMAND)
        );
    }

    #[test]
    fn hdmi_linked_runtime_renders_after_framebuffer_descriptor() {
        let _guard = test_guard();
        reset_runtime_for_test();
        let mut descriptor = descriptor_for(HOT_PATH_HDMI_TEXT, ROLE_DISPLAY);
        descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER;
        descriptor.framebuffer = DriverRuntimeFramebufferDescriptor {
            vaddr: 0x8000_0000,
            paddr: 0x3000_0000,
            width: 640,
            height: 480,
            pitch: 640 * 4,
            format: DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
        };
        let init = DriverTaskCommandRecord {
            sequence: 30,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_HDMI_TEXT,
            arg1: ROLE_DISPLAY,
            aux0: DRIVER_RUNTIME_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: core::mem::size_of::<DriverRuntimeInitDescriptor>() as u16,
                flags: 0,
            },
        };
        assert_eq!(
            service_runtime_init_for_test(init, descriptor),
            DriverTaskCompletionRecord::progress(30, HOT_PATH_HDMI_TEXT)
        );
        let frame = DriverTaskCommandRecord {
            sequence: 31,
            opcode: OPCODE_SUBMIT_FRAME,
            flags: 0,
            arg0: HOT_PATH_HDMI_TEXT,
            arg1: ROLE_DISPLAY,
            aux0: 0,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: 5,
                flags: 0,
            },
        };
        assert_eq!(
            service_command(0, frame),
            DriverTaskCompletionRecord::progress(31, 5)
        );
    }
}
