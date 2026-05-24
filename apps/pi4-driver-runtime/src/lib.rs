// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide fixed-ring runtime support for isolated Pi 4 driver images.
// Author: Lukas Bower

#![cfg_attr(target_os = "none", no_std)]
#![allow(unsafe_code)]

use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU32, Ordering},
};

use pi4_driver_abi::{
    DriverRuntimeInitDescriptor, DRIVER_RUNTIME_INIT_AUX, HOT_PATH_CYW43_WIFI, HOT_PATH_GENET_NIC,
    HOT_PATH_HDMI_TEXT, HOT_PATH_PCIE_ROOT, HOT_PATH_SDIO_HOST, HOT_PATH_SERIAL_CONSOLE,
    HOT_PATH_USB_KEYBOARD,
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
const OPCODE_IRQ: u16 = 2;
const OPCODE_SUBMIT_FRAME: u16 = 3;
const OPCODE_FLUSH: u16 = 4;
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
    if !opcode_matches_hot_path(command.opcode, command.arg0) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }

    match command.arg0 {
        HOT_PATH_SERIAL_CONSOLE => service_serial(command),
        HOT_PATH_USB_KEYBOARD
        | HOT_PATH_GENET_NIC
        | HOT_PATH_CYW43_WIFI
        | HOT_PATH_SDIO_HOST
        | HOT_PATH_PCIE_ROOT => service_unavailable_or_idle(command),
        HOT_PATH_HDMI_TEXT => service_hdmi_text(command),
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

fn service_hdmi_text(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
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
    DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE)
}

fn service_unavailable_or_idle(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if command.frame.len == 0 || command.opcode == OPCODE_IRQ || command.opcode == OPCODE_FLUSH {
        DriverTaskCompletionRecord::idle(command.sequence)
    } else {
        DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE)
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
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = pi4_driver_abi::DriverRuntimePageDescriptor::new(0x4000_0000);
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
}
