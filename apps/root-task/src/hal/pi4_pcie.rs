// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide HAL-owned BCM2711 PCIe/VL805 config proof for Pi 4 USB bring-up.
// Author: Lukas Bower

#![allow(unsafe_code)]

use core::cmp;
use core::ptr;
use core::sync::atomic::{fence, AtomicUsize, Ordering};

use super::{pi4_wifi, DeviceHal, HalError, KernelHal};
use crate::bootstrap::log as boot_log;
use crate::rust_alloc::vec::Vec;
use crate::sel4::{page_get_address, PAGE_BITS};

const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_MASK: usize = PAGE_SIZE - 1;
const MAP_EXACT_ATTEMPT_CAP: usize = 512;
const EXACT_MAP_LOG_STRIDE: usize = 64;

const BCM2711_PCIE_HOST_PHYS_BASE: usize = 0xFD50_0000;
const BCM2711_PCIE_MISC_MISC_CTRL: usize = 0x4008;
const BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LO: usize = 0x400c;
const BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_HI: usize = 0x4010;
const BCM2711_PCIE_MISC_RC_BAR1_CONFIG_LO: usize = 0x402c;
const BCM2711_PCIE_MISC_RC_BAR2_CONFIG_LO: usize = 0x4034;
const BCM2711_PCIE_MISC_RC_BAR2_CONFIG_HI: usize = 0x4038;
const BCM2711_PCIE_MISC_RC_BAR3_CONFIG_LO: usize = 0x403c;
const BCM2711_PCIE_MISC_PCIE_STATUS: usize = 0x4068;
const BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT: usize = 0x4070;
const BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_HI: usize = 0x4080;
const BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LIMIT_HI: usize = 0x4084;
const BCM2711_PCIE_MISC_HARD_PCIE_HARD_DEBUG: usize = 0x4204;
const BCM2711_PCIE_INTR2_CPU_CLR: usize = 0x4308;
const BCM2711_PCIE_INTR2_CPU_MASK_SET: usize = 0x4310;
const BCM2711_PCIE_MSI_INTR2_CLR: usize = 0x4508;
const BCM2711_PCIE_MSI_INTR2_MASK_SET: usize = 0x4510;
const BCM2711_PCIE_EXT_CFG_DATA: usize = 0x8000;
const BCM2711_PCIE_EXT_CFG_INDEX: usize = 0x9000;
const BCM2711_PCIE_RC_CFG_VENDOR_VENDOR_SPECIFIC_REG1: usize = 0x0188;
const BCM2711_PCIE_RC_CFG_PRIV1_ID_VAL3: usize = 0x043c;
const BCM2711_PCIE_RC_CFG_PRIV1_LINK_CAPABILITY: usize = 0x04dc;
const VL805_XHCI_USBCMD_OFFSET: usize = 0x0020;
const VL805_XHCI_DOORBELL0_OFFSET: usize = 0x0100;
const VL805_XHCI_DOORBELL_APERTURE_END: usize = 0x0200;
const VL805_XHCI_DOORBELL_STRIDE: usize = 4;
const VL805_XHCI_DOORBELL_FLUSH_STAGE: u16 = 0x031f;
const VL805_XHCI_RUN_FLUSH_STAGE: u16 = 0x02e5;
const VL805_FLUSH_LOG_RUNTIME_ERDP_LOW: usize = 1 << 0;
const VL805_FLUSH_LOG_RUNTIME_ERDP_HIGH: usize = 1 << 1;
const VL805_FLUSH_LOG_ENDPOINT_DOORBELL: usize = 1 << 2;
const BCM2711_PCIE_RGR1_SW_INIT_1: usize = 0x9210;

const PCIE_MISC_MISC_CTRL_SCB_ACCESS_EN_MASK: u32 = 0x1000;
const PCIE_MISC_MISC_CTRL_CFG_READ_UR_MODE_MASK: u32 = 0x2000;
const PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_MASK: u32 = 0x300000;
const PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_128: u32 = 0;
const PCIE_MISC_MISC_CTRL_SCB0_SIZE_MASK: u32 = 0xf8000000;
const PCIE_MISC_RC_BAR1_CONFIG_LO_SIZE_MASK: u32 = 0x1f;
const PCIE_MISC_RC_BAR2_CONFIG_LO_SIZE_MASK: u32 = 0x1f;
const PCIE_MISC_RC_BAR3_CONFIG_LO_SIZE_MASK: u32 = 0x1f;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT_LIMIT_MASK: u32 = 0xfff00000;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT_BASE_MASK: u32 = 0xfff0;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_HI_BASE_MASK: u32 = 0xff;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LIMIT_HI_LIMIT_MASK: u32 = 0xff;
const PCIE_RC_CFG_VENDOR_SPECIFIC_REG1_ENDIAN_MODE_BAR2_MASK: u32 = 0x0c;
const PCIE_RC_CFG_PRIV1_ID_VAL3_CLASS_CODE_MASK: u32 = 0x00ff_ffff;
const PCIE_RC_CFG_PRIV1_LINK_CAPABILITY_ASPM_SUPPORT_MASK: u32 = 0x0c00;
const PCIE_HARD_DEBUG_SERDES_IDDQ_MASK: u32 = 0x08000000;
const PCIE_RGR1_SW_INIT_1_INIT_MASK: u32 = 0x2;
const PCIE_RGR1_SW_INIT_1_PERST_MASK: u32 = 0x1;

static VL805_FLUSH_SUCCESS_LOGGED: AtomicUsize = AtomicUsize::new(0);

const BCM2711_PCIE_STATUS_PORT: u32 = 0x80;
const BCM2711_PCIE_STATUS_DL_ACTIVE: u32 = 0x20;
const BCM2711_PCIE_STATUS_PHY_LINK_UP: u32 = 0x10;

const VL805_PCI_DEV_ADDR: u32 = 0x0010_0000;
const VL805_PCI_VENDOR_ID: u16 = 0x1106;
const VL805_PCI_DEVICE_ID: u16 = 0x3483;
const VL805_EXPECTED_CLASS_CODE: u32 = 0x000c_0330;
const BCM2711_ROOT_VENDOR_ID: u16 = 0x14e4;
const BCM2711_ROOT_DEVICE_ID: u16 = 0x2711;
const BCM2711_ROOT_BRIDGE_CLASS_CODE: u32 = 0x0006_0400;

const PCI_CFG_VENDOR_DEVICE: usize = 0x00;
const PCI_CFG_COMMAND_STATUS: usize = 0x04;
const PCI_CFG_CLASS_REVISION: usize = 0x08;
const PCI_CFG_PRIMARY_BUS: usize = 0x18;
const PCI_CFG_IO_BASE_LIMIT: usize = 0x1c;
const PCI_CFG_MEMORY_BASE_LIMIT: usize = 0x20;
const PCI_CFG_PREFETCH_BASE_LIMIT: usize = 0x24;
const PCI_CFG_CAP_PTR: usize = 0x34;
const PCI_CFG_BAR0: usize = 0x10;
const PCI_CFG_BAR1: usize = 0x14;

const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;
const PCI_COMMAND_PARITY_ERROR_RESPONSE: u16 = 1 << 6;
const PCI_COMMAND_SERR_ENABLE: u16 = 1 << 8;
const PCI_COMMAND_INTERRUPT_DISABLE: u16 = 1 << 10;
const VL805_POLL_ONLY_COMMAND_REQUIRED: u16 = PCI_COMMAND_MEMORY_SPACE
    | PCI_COMMAND_BUS_MASTER
    | PCI_COMMAND_PARITY_ERROR_RESPONSE
    | PCI_COMMAND_SERR_ENABLE
    | PCI_COMMAND_INTERRUPT_DISABLE;
const PCI_STATUS_CAPABILITIES_LIST: u16 = 1 << 4;
const PCI_CAP_ID_MSI: u8 = 0x05;
const PCI_CAP_ID_EXP: u8 = 0x10;
const PCI_CAP_ID_MSIX: u8 = 0x11;
const PCI_CAP_NEXT_MASK: u8 = 0xfc;
const PCI_CAP_TRAVERSE_LIMIT: usize = 16;
const PCI_MSI_CONTROL_OFFSET: usize = 2;
const PCI_MSI_CONTROL_ENABLE: u16 = 1;
const PCI_MSIX_CONTROL_OFFSET: usize = 2;
const PCI_MSIX_CONTROL_MASKALL: u16 = 1 << 14;
const PCI_MSIX_CONTROL_ENABLE: u16 = 1 << 15;
const PCI_EXP_DEVCTL_OFFSET: usize = 8;
const PCI_EXP_DEVCTL_CORR_ERR: u16 = 1 << 0;
const PCI_EXP_DEVCTL_NON_FATAL_ERR: u16 = 1 << 1;
const PCI_EXP_DEVCTL_FATAL_ERR: u16 = 1 << 2;
const PCI_EXP_DEVCTL_UNSUP_REQ: u16 = 1 << 3;
const PCI_EXP_DEVCTL_RELAXED_ORDERING: u16 = 1 << 4;
const PCI_EXP_DEVCTL_MAX_PAYLOAD_MASK: u16 = 0x00e0;
const PCI_EXP_DEVCTL_MAX_PAYLOAD_128B: u16 = 0x0000;
const PCI_EXP_DEVCTL_NO_SNOOP: u16 = 1 << 11;
const PCI_EXP_DEVCTL_MAX_READ_REQ_MASK: u16 = 0x7000;
const PCI_EXP_DEVCTL_MAX_READ_REQ_512B: u16 = 0x2000;
const VL805_PCIE_DEVCTL_LINUX_CAPTURE: u16 = PCI_EXP_DEVCTL_CORR_ERR
    | PCI_EXP_DEVCTL_NON_FATAL_ERR
    | PCI_EXP_DEVCTL_FATAL_ERR
    | PCI_EXP_DEVCTL_UNSUP_REQ
    | PCI_EXP_DEVCTL_RELAXED_ORDERING
    | PCI_EXP_DEVCTL_MAX_PAYLOAD_128B
    | PCI_EXP_DEVCTL_NO_SNOOP
    | PCI_EXP_DEVCTL_MAX_READ_REQ_512B;
const VL805_PCIE_DEVCTL_COMMAND_PROOF: u16 = PCI_EXP_DEVCTL_CORR_ERR
    | PCI_EXP_DEVCTL_NON_FATAL_ERR
    | PCI_EXP_DEVCTL_FATAL_ERR
    | PCI_EXP_DEVCTL_UNSUP_REQ
    | PCI_EXP_DEVCTL_RELAXED_ORDERING
    | PCI_EXP_DEVCTL_MAX_PAYLOAD_128B
    | PCI_EXP_DEVCTL_NO_SNOOP
    | PCI_EXP_DEVCTL_MAX_READ_REQ_512B;

const RPI4_VL805_XHCI_MMIO: usize = 0x0000_0006_0000_0000;
const RPI4_PCIE_BUS_MMIO_WINDOW_BASE: usize = 0xC000_0000;
const RPI4_PCIE_BUS_MMIO_WINDOW_BASE_U32: u32 = 0xC000_0000;
const RPI4_PCIE_CPU_MMIO_WINDOW_BASE: usize = RPI4_VL805_XHCI_MMIO;
const RPI4_PCIE_BUS_MMIO_WINDOW_BYTES: usize = 0x4000_0000;
const RPI4_VL805_BRIDGE_MMIO_WINDOW_BYTES: u32 = 0x0010_0000;
const RPI4_PCIE_BRIDGE_BUS_NUMBERS: u32 = 0x0001_0100;
const RPI4_PCIE_BRIDGE_IO_BASE_LIMIT_DISABLED: u32 = 0;
const RPI4_PCIE_BRIDGE_PREFETCH_BASE_LIMIT_DISABLED: u32 = 0x0001_fff1;
const RPI4_PCIE_BRIDGE_COMMAND_REQUIRED: u16 = PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER;
// BCM2711 Pi 4 endpoint DMA uses the captured PCIe inbound bus alias:
// PCIe bus 0x00000004_00000000 maps to CPU physical 0 for 4 GiB.
const RPI4_PCIE_DMA_BUS_BASE: u64 = 0x0000_0004_0000_0000;
const RPI4_PCIE_DMA_CPU_BASE: u64 = 0;
const RPI4_PCIE_DMA_WINDOW_BYTES: u64 = 0x0000_0001_0000_0000;
const RPI4_PCIE_MMIO_WINDOW_SIZE: u64 = 0x4000_0000;
const PCIE_ROOT_DELAY_BASE_SPINS_PER_MS: usize = 50_000;
const PCIE_ROOT_DELAY_SPIN_SAFETY_MULTIPLIER: usize = 10;
const PCIE_SPINS_PER_MS: usize =
    PCIE_ROOT_DELAY_BASE_SPINS_PER_MS.saturating_mul(PCIE_ROOT_DELAY_SPIN_SAFETY_MULTIPLIER);
const PCIE_SHORT_SETTLE_SPINS: usize = PCIE_SPINS_PER_MS / 10;
const PCIE_POST_PERST_SETTLE_MS: usize = 100;
const PCIE_LINK_POLL_TOTAL_MS: usize = 100;
const PCIE_LINK_POLL_INTERVAL_MS: usize = 5;
const PCIE_LINK_POLL_ATTEMPTS: usize = PCIE_LINK_POLL_TOTAL_MS / PCIE_LINK_POLL_INTERVAL_MS;
const PCIE_POST_PERST_SETTLE_SPINS: usize =
    PCIE_POST_PERST_SETTLE_MS.saturating_mul(PCIE_SPINS_PER_MS);
const PCIE_LINK_POLL_SPINS: usize = PCIE_LINK_POLL_INTERVAL_MS.saturating_mul(PCIE_SPINS_PER_MS);
const VL805_POST_PCIE_RESET_NOTIFY_SETTLE_MS: usize = 20;
const VL805_POST_PCIE_RESET_NOTIFY_SETTLE_SPINS: usize =
    VL805_POST_PCIE_RESET_NOTIFY_SETTLE_MS.saturating_mul(PCIE_SPINS_PER_MS);
const PCIE_EXT_CFG_SELECT_SETTLE_SPINS: usize = 1_024;
const PCIE_EXT_CFG_SELECTOR_RETRIES: usize = 2;
const VL805_XHCI_PORTSC_BASE_OFFSET: usize = 0x420;
const VL805_XHCI_PORTSC_STRIDE: usize = 0x10;
const VL805_XHCI_PORT_REGISTER_MMIO_LIMIT: usize = 0x1_0000;

static PCIE_ROOT_CFG_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_STATUS_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_EXT_DATA_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_EXT_INDEX_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_ROOT_INIT_ATTEMPTED: AtomicUsize = AtomicUsize::new(0);
static PCIE_ROOT_INIT_POST_MAILBOX_ATTEMPTED: AtomicUsize = AtomicUsize::new(0);
static PCIE_LINK_AND_RC_READY_PROVEN: AtomicUsize = AtomicUsize::new(0);
static PCIE_IRQ_SOURCES_MASKED_PROVEN: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_HEAD: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_TAIL: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_SUBMITTED: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_RING_SERVICED: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_ROOT_FALLBACKS: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_REJECTED: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_LAST_OP_STAGE: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_LAST_OFFSET: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_LAST_VALUE: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_LAST_RESULT: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_NON_ACCEPTANCE_LOGGED: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_REPLAY_PENDING_LOGGED: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_ENGINE_READY: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_ENGINE_INIT_FAIL_LOGGED: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_FIRST_TURN_LOGGED: AtomicUsize = AtomicUsize::new(0);
static PCIE_OWNER_QUEUE_NO_REPLY_LOGGED: AtomicUsize = AtomicUsize::new(0);

const PCIE_OWNER_QUEUE_RECORD_VERSION: u16 = 1;
const PCIE_OWNER_QUEUE_DEPTH: usize = 32;
const PCIE_OWNER_QUEUE_FLAG_FIXED_RING_COMMAND: u16 = 1 << 0;
const PCIE_OWNER_QUEUE_FLAG_ROOT_MMIO_EXEC: u16 = 1 << 1;
const PCIE_OWNER_QUEUE_FLAGS: u16 = PCIE_OWNER_QUEUE_FLAG_FIXED_RING_COMMAND
    | PCIE_OWNER_QUEUE_FLAG_ROOT_MMIO_EXEC
    | super::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE;
const PCIE_OWNER_OP_PORT_READ: u16 = 1;
const PCIE_OWNER_OP_PORT_WRITE: u16 = 2;
const PCIE_OWNER_OP_POSTED_WRITE_FLUSH: u16 = 3;
const PCIE_OWNER_QUEUE_NON_ACCEPTANCE_REASON: &str = "root-mmio-exec";

/// Fixed-layout PCIe/VL805 owner-queue accounting record.
///
/// This is a bounded primitive mirror of the bus-owner queue state. It is not
/// registered as final owner-state proof because the live MMIO read/write still
/// executes in the root mapping after the pointer-free service turn.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pi4PcieOwnerQueueRecord {
    version: u16,
    flags: u16,
    depth: u16,
    non_acceptance_reason: u16,
    head: u32,
    tail: u32,
    submitted: u32,
    ring_serviced: u32,
    root_fallbacks: u32,
    rejected: u32,
    last_op: u16,
    last_stage: u16,
    last_offset: u32,
    last_value: u32,
    last_result: u32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl Pi4PcieOwnerQueueRecord {
    const fn non_acceptance_reason(self) -> &'static str {
        let _ = self;
        PCIE_OWNER_QUEUE_NON_ACCEPTANCE_REASON
    }

    const fn acceptance_eligible(self) -> bool {
        false
    }
}

fn pcie_owner_queue_result_word(ring_serviced: bool, root_fallback: bool) -> usize {
    (ring_serviced as usize) | ((root_fallback as usize) << 1)
}

fn pcie_owner_queue_log_non_acceptance_once() {
    if PCIE_OWNER_QUEUE_NON_ACCEPTANCE_LOGGED.swap(1, Ordering::AcqRel) == 0 {
        boot_log::force_uart_line(
            "[local-seat] pcie owner-state non-acceptance hot_path=pcie-root queue=fixed-ring-record reason=root-mmio-exec action=keep-owner-proof-open",
        );
    }
}

#[cfg(feature = "kernel")]
fn pcie_owner_queue_ring_service_completion(
    op: u16,
    stage: u16,
    offset: usize,
    value: u32,
) -> Option<super::driver_task::DriverTaskCompletionRecord> {
    let contract = super::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT;
    let _ = super::driver_task::register_pi4_bus_ring_service(contract);
    if !super::driver_task::ensure_deferred_runtime_init_descriptor(
        contract,
        super::driver_task::DriverTaskHotPath::PcieRoot,
    ) {
        if PCIE_OWNER_QUEUE_REPLAY_PENDING_LOGGED.swap(1, Ordering::AcqRel) == 0 {
            super::driver_task::emit_driver_task_resource_init_status(
                contract,
                super::driver_task::DriverTaskHotPath::PcieRoot,
                "pcie-runtime-replay",
                "pending",
                None,
            );
        }
        return None;
    }
    if !pcie_owner_queue_ensure_engine_ready(contract) {
        return None;
    }
    let value_bytes = value.to_le_bytes();
    let frame = if op == PCIE_OWNER_OP_PORT_WRITE {
        match super::driver_task::describe_driver_task_ring_frame(&value_bytes, 0) {
            Some(frame) => frame,
            None => {
                super::driver_task::emit_driver_task_resource_init_status(
                    contract,
                    super::driver_task::DriverTaskHotPath::PcieRoot,
                    "pcie-owner-turn",
                    "stage-failed",
                    None,
                );
                return None;
            }
        }
    } else {
        super::driver_task::DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        }
    };
    let mut command = super::driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        super::driver_task::DriverTaskHotPath::PcieRoot,
        super::driver_task::DriverTaskBudgetGrant::from_contract(contract),
        frame,
    );
    command.aux0 = ((op as u32) << 16) | u32::from(stage);
    command.aux1 = offset as u32;
    if !physical_pi_pcie_owner_ring_required() {
        command.flags = PCIE_OWNER_QUEUE_FLAGS;
        command.frame.flags = PCIE_OWNER_QUEUE_FLAGS;
    }
    let completion = if op == PCIE_OWNER_OP_PORT_WRITE {
        let staging_segments = [super::driver_task::DriverTaskStagingSegment::ring_frame(
            &value_bytes,
            0,
        )];
        super::driver_task::run_driver_task_ring_service_staged(
            contract,
            command,
            &staging_segments,
        )
    } else {
        super::driver_task::run_driver_task_ring_service(contract, command)
    };
    if PCIE_OWNER_QUEUE_FIRST_TURN_LOGGED.swap(1, Ordering::AcqRel) == 0
        || (completion.is_none() && PCIE_OWNER_QUEUE_NO_REPLY_LOGGED.swap(1, Ordering::AcqRel) == 0)
    {
        let status = match completion {
            Some(done)
                if done.code != super::driver_task::DriverTaskCompletionCode::Fault.as_u16() =>
            {
                "ready"
            }
            Some(_) => "fault",
            None => "no-reply",
        };
        super::driver_task::emit_driver_task_resource_init_status(
            contract,
            super::driver_task::DriverTaskHotPath::PcieRoot,
            "pcie-owner-turn",
            status,
            completion,
        );
    }
    completion
}

#[cfg(feature = "kernel")]
fn pcie_owner_queue_ensure_engine_ready(contract: super::driver_task::DriverTaskContract) -> bool {
    if PCIE_OWNER_QUEUE_ENGINE_READY.load(Ordering::Acquire) != 0 {
        return true;
    }

    let command = super::driver_task::runtime_engine_init_command(
        super::driver_task::DriverTaskHotPath::PcieRoot,
        super::driver_task::DriverTaskBudgetGrant::from_contract(contract),
    );
    let completion = super::driver_task::run_driver_task_ring_service(contract, command);
    let ready = completion.is_some_and(|completion| {
        completion.code == super::driver_task::DriverTaskCompletionCode::Progress.as_u16()
            && completion.result == 1
    });
    let status = match completion {
        Some(done)
            if done.code == super::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && done.result == 1 =>
        {
            "ready"
        }
        Some(_) => "fault",
        None => "no-reply",
    };
    if ready {
        PCIE_OWNER_QUEUE_ENGINE_READY.store(1, Ordering::Release);
        super::driver_task::emit_driver_task_resource_init_status(
            contract,
            super::driver_task::DriverTaskHotPath::PcieRoot,
            "pcie-engine-init",
            status,
            completion,
        );
        true
    } else {
        if PCIE_OWNER_QUEUE_ENGINE_INIT_FAIL_LOGGED.swap(1, Ordering::AcqRel) == 0 {
            super::driver_task::emit_driver_task_resource_init_status(
                contract,
                super::driver_task::DriverTaskHotPath::PcieRoot,
                "pcie-engine-init",
                status,
                completion,
            );
        }
        false
    }
}

#[cfg(feature = "kernel")]
fn pcie_owner_queue_ring_service_turn(op: u16, stage: u16, offset: usize, value: u32) -> bool {
    pcie_owner_queue_ring_service_completion(op, stage, offset, value).is_some_and(|completion| {
        let ok = completion.code != super::driver_task::DriverTaskCompletionCode::Fault.as_u16();
        if ok {
            let _ = super::driver_task::register_driver_task_runtime_owner_state(
                super::driver_task::DriverTaskHotPath::PcieRoot,
            );
        }
        ok
    })
}

#[cfg(feature = "kernel")]
fn pcie_owner_queue_runtime_read(offset: usize) -> Option<u32> {
    pcie_owner_queue_ring_service_completion(PCIE_OWNER_OP_PORT_READ, 0, offset, 0)
        .filter(|completion| {
            completion.code != super::driver_task::DriverTaskCompletionCode::Fault.as_u16()
        })
        .map(|completion| {
            let _ = super::driver_task::register_driver_task_runtime_owner_state(
                super::driver_task::DriverTaskHotPath::PcieRoot,
            );
            completion.result
        })
}

#[cfg(feature = "kernel")]
fn pcie_owner_queue_runtime_write(op: u16, stage: u16, offset: usize, value: u32) -> bool {
    pcie_owner_queue_ring_service_completion(op, stage, offset, value).is_some_and(|completion| {
        let ok = completion.code != super::driver_task::DriverTaskCompletionCode::Fault.as_u16();
        if ok {
            let _ = super::driver_task::register_driver_task_runtime_owner_state(
                super::driver_task::DriverTaskHotPath::PcieRoot,
            );
        }
        ok
    })
}

#[cfg(not(feature = "kernel"))]
fn pcie_owner_queue_ring_service_turn(_op: u16, _stage: u16, _offset: usize, _value: u32) -> bool {
    false
}

#[cfg(not(feature = "kernel"))]
fn pcie_owner_queue_runtime_read(_offset: usize) -> Option<u32> {
    None
}

#[cfg(not(feature = "kernel"))]
fn pcie_owner_queue_runtime_write(_op: u16, _stage: u16, _offset: usize, _value: u32) -> bool {
    false
}

fn pcie_owner_queue_submit(op: u16, stage: u16, offset: usize, value: u32) -> bool {
    if offset > u32::MAX as usize {
        PCIE_OWNER_QUEUE_REJECTED.fetch_add(1, Ordering::AcqRel);
        return false;
    }

    let submitted = PCIE_OWNER_QUEUE_SUBMITTED
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    PCIE_OWNER_QUEUE_HEAD.store(submitted % PCIE_OWNER_QUEUE_DEPTH, Ordering::Release);
    PCIE_OWNER_QUEUE_LAST_OP_STAGE.store(
        ((op as usize) << 16) | usize::from(stage),
        Ordering::Release,
    );
    PCIE_OWNER_QUEUE_LAST_OFFSET.store(offset, Ordering::Release);
    PCIE_OWNER_QUEUE_LAST_VALUE.store(value as usize, Ordering::Release);

    let ring_serviced = pcie_owner_queue_ring_service_turn(op, stage, offset, value);
    if ring_serviced {
        PCIE_OWNER_QUEUE_RING_SERVICED.fetch_add(1, Ordering::AcqRel);
        PCIE_OWNER_QUEUE_TAIL.store(submitted % PCIE_OWNER_QUEUE_DEPTH, Ordering::Release);
        PCIE_OWNER_QUEUE_LAST_RESULT
            .store(pcie_owner_queue_result_word(true, false), Ordering::Release);
    } else {
        PCIE_OWNER_QUEUE_ROOT_FALLBACKS.fetch_add(1, Ordering::AcqRel);
        PCIE_OWNER_QUEUE_LAST_RESULT
            .store(pcie_owner_queue_result_word(false, true), Ordering::Release);
        pcie_owner_queue_log_non_acceptance_once();
    }
    ring_serviced
}

#[inline]
fn physical_pi_pcie_owner_ring_required() -> bool {
    super::driver_task::physical_pi_driver_task_only_owner_state_active()
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn pi4_pcie_owner_queue_record() -> Pi4PcieOwnerQueueRecord {
    let op_stage = PCIE_OWNER_QUEUE_LAST_OP_STAGE.load(Ordering::Acquire);
    Pi4PcieOwnerQueueRecord {
        version: PCIE_OWNER_QUEUE_RECORD_VERSION,
        flags: PCIE_OWNER_QUEUE_FLAGS,
        depth: PCIE_OWNER_QUEUE_DEPTH as u16,
        non_acceptance_reason: 1,
        head: PCIE_OWNER_QUEUE_HEAD.load(Ordering::Acquire) as u32,
        tail: PCIE_OWNER_QUEUE_TAIL.load(Ordering::Acquire) as u32,
        submitted: PCIE_OWNER_QUEUE_SUBMITTED.load(Ordering::Acquire) as u32,
        ring_serviced: PCIE_OWNER_QUEUE_RING_SERVICED.load(Ordering::Acquire) as u32,
        root_fallbacks: PCIE_OWNER_QUEUE_ROOT_FALLBACKS.load(Ordering::Acquire) as u32,
        rejected: PCIE_OWNER_QUEUE_REJECTED.load(Ordering::Acquire) as u32,
        last_op: (op_stage >> 16) as u16,
        last_stage: (op_stage & 0xffff) as u16,
        last_offset: PCIE_OWNER_QUEUE_LAST_OFFSET.load(Ordering::Acquire) as u32,
        last_value: PCIE_OWNER_QUEUE_LAST_VALUE.load(Ordering::Acquire) as u32,
        last_result: PCIE_OWNER_QUEUE_LAST_RESULT.load(Ordering::Acquire) as u32,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pi4PcieProofPhase {
    Initial,
    PostMailboxReset,
}

impl Pi4PcieProofPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::PostMailboxReset => "post-mailbox-reset",
        }
    }

    const fn powers_vl805_usb_hcd(self) -> bool {
        matches!(self, Self::Initial)
    }

    const fn reloads_vl805_firmware_after_perst(self) -> bool {
        matches!(self, Self::PostMailboxReset)
    }

    fn root_init_latch(self) -> &'static AtomicUsize {
        match self {
            Self::Initial => &PCIE_ROOT_INIT_ATTEMPTED,
            Self::PostMailboxReset => &PCIE_ROOT_INIT_POST_MAILBOX_ATTEMPTED,
        }
    }
}

fn finish_pi4_pcie_root_init_attempt(phase: Pi4PcieProofPhase, ready: bool) {
    if !ready {
        phase.root_init_latch().store(0, Ordering::Release);
    }
}

fn notify_vl805_reset_after_pcie_ready(
    hal: &mut KernelHal<'_>,
    phase: Pi4PcieProofPhase,
    stage: &'static str,
    reason: &'static str,
) -> Result<(), HalError> {
    if !phase.reloads_vl805_firmware_after_perst() {
        return Ok(());
    }

    let mut begin = heapless::String::<256>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut begin,
        format_args!(
            "[local-seat] vl805 reset ownership=runtime-owned stage={stage} action=mailbox-notify reason={reason} settle_ms={}",
            VL805_POST_PCIE_RESET_NOTIFY_SETTLE_MS
        ),
    );
    boot_log::force_uart_line(begin.as_str());

    let result = match pi4_wifi::notify_vl805_reset(hal) {
        Ok(result) => result,
        Err(err) => {
            finish_pi4_pcie_root_init_attempt(phase, false);
            let mut fail = heapless::String::<240>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut fail,
                format_args!(
                    "[local-seat] vl805 reset ownership=runtime-unconfirmed stage={stage} action=mailbox-notify-failed err={err}"
                ),
            );
            boot_log::force_uart_line(fail.as_str());
            return Err(err);
        }
    };

    pcie_spin_delay(VL805_POST_PCIE_RESET_NOTIFY_SETTLE_SPINS);
    let result_label = match result {
        pi4_wifi::Vl805ResetNotifyResult::Acked => "mailbox-notify+settle",
    };
    let mut done = heapless::String::<256>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut done,
        format_args!(
            "[local-seat] vl805 reset ownership=runtime-owned stage={stage} detail={result_label} action=mailbox-notify-complete"
        ),
    );
    boot_log::force_uart_line(done.as_str());
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pi4Vl805PcieProof {
    pub status: u32,
    pub config_virt: usize,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u32,
    pub command_before: u16,
    pub command_after: u16,
    pub bar0: u32,
    pub bar1: u32,
    pub mmio: usize,
    pub msi_control_before: Option<u16>,
    pub msi_control_after: Option<u16>,
    pub msix_control_before: Option<u16>,
    pub msix_control_after: Option<u16>,
    pub pcie_devctl_before: Option<u16>,
    pub pcie_devctl_after: Option<u16>,
}

impl Pi4Vl805PcieProof {
    #[must_use]
    pub const fn msi_disabled(self) -> bool {
        match self.msi_control_after {
            Some(control) => vl805_msi_control_disabled(control),
            None => true,
        }
    }

    #[must_use]
    pub const fn msix_quiesced(self) -> bool {
        match self.msix_control_after {
            Some(control) => vl805_msix_control_quiesced(control),
            None => true,
        }
    }

    #[must_use]
    pub const fn interrupt_modes_quiesced(self) -> bool {
        self.msi_disabled() && self.msix_quiesced()
    }

    #[must_use]
    pub const fn pcie_device_control_ready(self) -> bool {
        match self.pcie_devctl_after {
            Some(control) => vl805_pcie_devctl_ready(control),
            None => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Vl805InterruptModeProof {
    msi_control_before: Option<u16>,
    msi_control_after: Option<u16>,
    msix_control_before: Option<u16>,
    msix_control_after: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Vl805PcieDeviceControlProof {
    control_before: Option<u16>,
    control_after: Option<u16>,
}

impl<'a> KernelHal<'a> {
    pub fn prove_pi4_vl805_pcie_ownership(&mut self) -> Result<Pi4Vl805PcieProof, HalError> {
        prove_pi4_vl805_pcie_ownership(self, Pi4PcieProofPhase::Initial)
    }

    pub fn prove_pi4_vl805_pcie_ownership_after_mailbox_reset(
        &mut self,
    ) -> Result<Pi4Vl805PcieProof, HalError> {
        prove_pi4_vl805_pcie_ownership(self, Pi4PcieProofPhase::PostMailboxReset)
    }
}

#[must_use]
pub fn pi4_pcie_link_and_rc_ready_proven() -> bool {
    PCIE_LINK_AND_RC_READY_PROVEN.load(Ordering::Acquire) != 0
}

#[must_use]
pub fn pi4_pcie_irq_sources_masked_proven() -> bool {
    PCIE_IRQ_SOURCES_MASKED_PROVEN.load(Ordering::Acquire) != 0
}

pub fn invalidate_pi4_pcie_runtime_proofs(reason: &'static str) {
    PCIE_LINK_AND_RC_READY_PROVEN.store(0, Ordering::Release);
    PCIE_IRQ_SOURCES_MASKED_PROVEN.store(0, Ordering::Release);
    let mut line = heapless::String::<160>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!("[local-seat] vl805 pcie proof invalidated reason={reason}"),
    );
    boot_log::force_uart_line(line.as_str());
}

#[must_use]
pub const fn vl805_post_mailbox_ext_cfg_retry_needed(
    mmio: usize,
    high_bar_mmio: usize,
    fresh_runtime_ready: bool,
    runtime_touch_enabled: bool,
) -> bool {
    runtime_touch_enabled && mmio == high_bar_mmio && !fresh_runtime_ready
}

const fn pcie_root_ready_fast_path_allowed(_phase: Pi4PcieProofPhase, _status: u32) -> bool {
    // Raw BCM2711 status bits are only advisory before Cohesix has refreshed the
    // root window and proved the exact VL805 config tuple. Recent Pi 4 boots
    // exposed status values with PORT/DL/PHY bits set while EXT_CFG still read
    // root-port garbage, so status alone must never skip the controlled root
    // init path.
    false
}

#[inline]
const fn vl805_xhci_port_register_offset_valid(offset: usize, port: u8, max_ports: u8) -> bool {
    if max_ports == 0 || port >= max_ports || (offset & 0x3) != 0 {
        return false;
    }
    let expected = VL805_XHCI_PORTSC_BASE_OFFSET
        .saturating_add((port as usize).saturating_mul(VL805_XHCI_PORTSC_STRIDE));
    offset == expected
        && offset <= VL805_XHCI_PORT_REGISTER_MMIO_LIMIT - core::mem::size_of::<u32>()
}

#[inline]
fn vl805_xhci_port_register_addr(mmio_virt: usize, offset: usize) -> Option<usize> {
    if mmio_virt == 0 {
        return None;
    }
    mmio_virt.checked_add(offset)
}

/// Reads a VL805 xHCI root-port register through the Pi 4 HAL policy boundary.
pub fn vl805_xhci_port_read32(mmio_virt: usize, offset: usize, port: u8, max_ports: u8) -> u32 {
    if !vl805_xhci_port_register_offset_valid(offset, port, max_ports) {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 xhci port read rejected port={} max_ports={} offset=0x{offset:04x}",
                port.saturating_add(1),
                max_ports,
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return 0;
    }
    let Some(addr) = vl805_xhci_port_register_addr(mmio_virt, offset) else {
        boot_log::force_uart_line("[local-seat] vl805 xhci port read rejected reason=no-mmio");
        return 0;
    };
    if physical_pi_pcie_owner_ring_required() {
        if let Some(value) = pcie_owner_queue_runtime_read(offset) {
            return value;
        }
        boot_log::force_uart_line(
            "[local-seat] vl805 xhci port read rejected reason=pcie-owner-ring-unavailable action=fail-closed",
        );
        return 0;
    }
    if !pcie_owner_queue_submit(PCIE_OWNER_OP_PORT_READ, 0, offset, 0) {
        pcie_owner_queue_log_non_acceptance_once();
    }
    fence(Ordering::SeqCst);
    let ptr = addr as *const u32;
    // SAFETY: the caller-installed xHCI hook supplies a live device mapping
    // for the VL805 MMIO window, and the offset was bounded to the root-port
    // register aperture before this volatile device read.
    let value = unsafe { ptr::read_volatile(ptr) };
    fence(Ordering::SeqCst);
    value
}

/// Writes a VL805 xHCI root-port register through the Pi 4 HAL policy boundary.
pub fn vl805_xhci_port_write32(
    mmio_virt: usize,
    offset: usize,
    port: u8,
    max_ports: u8,
    value: u32,
) {
    if !vl805_xhci_port_register_offset_valid(offset, port, max_ports) {
        let mut line = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 xhci port write rejected port={} max_ports={} offset=0x{offset:04x} value=0x{value:08x}",
                port.saturating_add(1),
                max_ports,
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return;
    }
    let Some(addr) = vl805_xhci_port_register_addr(mmio_virt, offset) else {
        boot_log::force_uart_line("[local-seat] vl805 xhci port write rejected reason=no-mmio");
        return;
    };
    if physical_pi_pcie_owner_ring_required() {
        if pcie_owner_queue_runtime_write(PCIE_OWNER_OP_PORT_WRITE, 0, offset, value) {
            return;
        }
        boot_log::force_uart_line(
            "[local-seat] vl805 xhci port write rejected reason=pcie-owner-ring-unavailable action=fail-closed",
        );
        return;
    }
    if !pcie_owner_queue_submit(PCIE_OWNER_OP_PORT_WRITE, 0, offset, value) {
        pcie_owner_queue_log_non_acceptance_once();
    }
    fence(Ordering::SeqCst);
    let ptr = addr as *mut u32;
    // SAFETY: the caller-installed xHCI hook supplies a live device mapping
    // for the VL805 MMIO window, and the offset was bounded to the root-port
    // register aperture before this volatile device write.
    unsafe { ptr::write_volatile(ptr, value) };
    fence(Ordering::SeqCst);
}

const fn vl805_xhci_flush_is_doorbell(stage: u16, offset: usize) -> bool {
    stage == VL805_XHCI_DOORBELL_FLUSH_STAGE
        && offset >= VL805_XHCI_DOORBELL0_OFFSET
        && offset < VL805_XHCI_DOORBELL_APERTURE_END
        && (offset - VL805_XHCI_DOORBELL0_OFFSET) % VL805_XHCI_DOORBELL_STRIDE == 0
}

const fn vl805_xhci_flush_is_run_stage(stage: u16) -> bool {
    matches!(stage, VL805_XHCI_RUN_FLUSH_STAGE | 0x035c | 0x03eb)
}

const fn vl805_xhci_flush_is_run_write(stage: u16, offset: usize) -> bool {
    vl805_xhci_flush_is_run_stage(stage) && offset == VL805_XHCI_USBCMD_OFFSET
}

const fn vl805_xhci_flush_skips_bar_drain(stage: u16, offset: usize) -> bool {
    vl805_xhci_flush_is_doorbell(stage, offset) || vl805_xhci_flush_is_run_write(stage, offset)
}

const fn vl805_xhci_flush_live_proof_failure(
    link_and_rc_ready: bool,
    irq_sources_masked: bool,
    command_status: u32,
) -> Option<&'static str> {
    if !link_and_rc_ready {
        Some("link-root-unproven")
    } else if !irq_sources_masked {
        Some("irq-source-mask-unproven")
    } else if !vl805_command_ownership_ready(command_status as u16) {
        Some("command-ownership")
    } else {
        None
    }
}

fn cached_pcie_status_readback() -> u32 {
    let status_page = PCIE_STATUS_PAGE_VIRT.load(Ordering::Acquire);
    if status_page == 0 {
        return u32::MAX;
    }
    match same_page_reg_virt(status_page, BCM2711_PCIE_MISC_PCIE_STATUS) {
        Ok(status_reg) => mmio_read_u32(status_reg),
        Err(_) => u32::MAX,
    }
}

const fn vl805_xhci_flush_stage_role(stage: u16, offset: usize) -> &'static str {
    match (stage, offset) {
        (0x0118, 0x0c08) => "brcm-axiwra",
        (0x0119, 0x0c0c) => "brcm-axirda",
        (0x03fe, _) => "runtime-erdp-ack-low",
        (0x03ff, _) => "runtime-erdp-ack-high",
        (0x03b7, _) => "ring-publish-erdp-low",
        (0x03b8, _) => "ring-publish-erdp-high",
        (0x031f, 0x0100) => "command-doorbell",
        (_, 0x0100) => "doorbell0",
        (_, offset)
            if offset > VL805_XHCI_DOORBELL0_OFFSET
                && offset < VL805_XHCI_DOORBELL_APERTURE_END
                && (offset - VL805_XHCI_DOORBELL0_OFFSET) % VL805_XHCI_DOORBELL_STRIDE == 0 =>
        {
            "endpoint-doorbell"
        }
        (_, VL805_XHCI_USBCMD_OFFSET) if vl805_xhci_flush_is_run_write(stage, offset) => {
            "run-command"
        }
        (_, _) => "generic-mmio",
    }
}

const fn vl805_xhci_flush_success_log_mask(stage: u16, offset: usize) -> Option<usize> {
    match (stage, offset) {
        (0x03fe, _) => Some(VL805_FLUSH_LOG_RUNTIME_ERDP_LOW),
        (0x03ff, _) => Some(VL805_FLUSH_LOG_RUNTIME_ERDP_HIGH),
        (_, offset)
            if offset > VL805_XHCI_DOORBELL0_OFFSET
                && offset < VL805_XHCI_DOORBELL_APERTURE_END
                && (offset - VL805_XHCI_DOORBELL0_OFFSET) % VL805_XHCI_DOORBELL_STRIDE == 0 =>
        {
            Some(VL805_FLUSH_LOG_ENDPOINT_DOORBELL)
        }
        (_, _) => None,
    }
}

fn vl805_xhci_flush_should_log_success(stage: u16, offset: usize) -> bool {
    match vl805_xhci_flush_success_log_mask(stage, offset) {
        Some(mask) => VL805_FLUSH_SUCCESS_LOGGED.fetch_or(mask, Ordering::AcqRel) & mask == 0,
        None => true,
    }
}

#[inline(always)]
fn pi4_pcie_mmio_write_barrier() {
    fence(Ordering::SeqCst);
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: `dmb sy` orders prior normal/cache-maintenance writes before
        // subsequent device/config reads used as posted-write drains.
        unsafe { core::arch::asm!("dmb sy", options(nostack, preserves_flags)) };
    }
}

#[inline(always)]
fn pi4_pcie_mmio_read_barrier() {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: `dmb sy` matches the xHCI MMIO read barrier and orders the
        // HAL-owned config readback before later proof decisions.
        unsafe { core::arch::asm!("dmb sy", options(nostack, preserves_flags)) };
    }
    fence(Ordering::SeqCst);
}

/// Flushes posted VL805 xHCI MMIO writes through HAL-owned read drains.
pub fn vl805_xhci_flush_posted_write(
    mmio_virt: usize,
    offset: usize,
    value: u32,
    stage: u16,
) -> bool {
    let role = vl805_xhci_flush_stage_role(stage, offset);
    if physical_pi_pcie_owner_ring_required() {
        if pcie_owner_queue_runtime_write(PCIE_OWNER_OP_POSTED_WRITE_FLUSH, stage, offset, value) {
            return true;
        }
        boot_log::force_uart_line(
            "[local-seat] vl805 posted-write flush rejected reason=pcie-owner-ring-unavailable action=fail-closed",
        );
        return false;
    }
    if !pcie_owner_queue_submit(PCIE_OWNER_OP_POSTED_WRITE_FLUSH, stage, offset, value) {
        pcie_owner_queue_log_non_acceptance_once();
    }
    let config_page = PCIE_EXT_DATA_PAGE_VIRT.load(Ordering::Acquire);
    let index_page = PCIE_EXT_INDEX_PAGE_VIRT.load(Ordering::Acquire);
    if config_page == 0 || index_page == 0 {
        let mut line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush skipped stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} reason=no-ext-cfg mmio=0x{mmio_virt:016x}"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return false;
    }
    let Ok(config_virt) = same_page_reg_virt(config_page, BCM2711_PCIE_EXT_CFG_DATA) else {
        boot_log::force_uart_line(
            "[local-seat] vl805 posted-write flush skipped reason=bad-ext-cfg-data",
        );
        return false;
    };
    let Ok(index_reg) = same_page_reg_virt(index_page, BCM2711_PCIE_EXT_CFG_INDEX) else {
        boot_log::force_uart_line(
            "[local-seat] vl805 posted-write flush skipped reason=bad-ext-cfg-index",
        );
        return false;
    };
    pi4_pcie_mmio_write_barrier();
    let selected = bcm2711_ext_cfg_select(index_reg);
    let command_status = pci_cfg_read_u32(config_virt, PCI_CFG_COMMAND_STATUS);
    let bar_drain_skipped = vl805_xhci_flush_skips_bar_drain(stage, offset);
    pi4_pcie_mmio_read_barrier();
    if !vl805_ext_cfg_flush_read_valid(selected, command_status) {
        let mut line = heapless::String::<384>::new();
        let reason = if selected != VL805_PCI_DEV_ADDR {
            "selector"
        } else if vl805_ext_cfg_selector_echo(command_status) {
            "selector-echo"
        } else {
            "command-status"
        };
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush failed stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} reason={reason} mmio=0x{mmio_virt:016x} source=hal-ext-cfg"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return false;
    }
    if let Some(reason) = vl805_xhci_flush_live_proof_failure(
        pi4_pcie_link_and_rc_ready_proven(),
        pi4_pcie_irq_sources_masked_proven(),
        command_status,
    ) {
        let mut line = heapless::String::<384>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush failed stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} reason={reason} mmio=0x{mmio_virt:016x} source=hal-ext-cfg"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return false;
    }
    let Ok(bar0_readback) = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR0) else {
        let mut line = heapless::String::<320>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush failed stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} reason=bar0-readback mmio=0x{mmio_virt:016x} source=hal-ext-cfg"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return false;
    };
    let Ok(bar1_readback) = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR1) else {
        let mut line = heapless::String::<320>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush failed stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} reason=bar1-readback mmio=0x{mmio_virt:016x} source=hal-ext-cfg"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return false;
    };
    let bridge_status = cached_pcie_status_readback();
    if bar_drain_skipped
        && translate_vl805_pci_bar_to_cpu_mmio(bar0_readback, bar1_readback)
            != Some(RPI4_VL805_XHCI_MMIO)
    {
        let mut line = heapless::String::<512>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush failed stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} bar=0x{bar0_readback:08x}/0x{bar1_readback:08x} bridge_status=0x{bridge_status:08x} reason=bar-readback mmio=0x{mmio_virt:016x} source=hal-ext-cfg"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return false;
    }
    if !vl805_xhci_flush_should_log_success(stage, offset) {
        return true;
    }
    let mut line = heapless::String::<512>::new();
    if bar_drain_skipped {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} bar=0x{bar0_readback:08x}/0x{bar1_readback:08x} bridge_status=0x{bridge_status:08x} bar_drain=skipped drain=ext-cfg-command+bar+bridge-status reason=pi4-run-doorbell-xhci-read-toxic mmio=0x{mmio_virt:016x} source=hal-ext-cfg"
            ),
        );
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush stage=0x{stage:04x} role={role} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} source=hal-ext-cfg"
            ),
        );
    }
    boot_log::force_uart_line(line.as_str());
    true
}

fn prove_pi4_vl805_pcie_ownership(
    hal: &mut KernelHal<'_>,
    phase: Pi4PcieProofPhase,
) -> Result<Pi4Vl805PcieProof, HalError> {
    if phase.powers_vl805_usb_hcd() {
        pi4_wifi::power_on_vl805_usb_hcd(hal)?;
    } else {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie power stage={} action=skip reason=powered-before-vl805-reset-notify",
                phase.label()
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    // seL4 device untyped retyping is monotonic. Map the BCM2711 PCIe register
    // pages in ascending physical order so root-port config, status, EXT_CFG,
    // and SW_INIT remain exactly mappable in one boot.
    let root_cfg_page = map_pcie_reg_page_cached(hal, PCI_CFG_VENDOR_DEVICE, "pi4-pcie-root-cfg")?;
    let status_page =
        map_pcie_reg_page_cached(hal, BCM2711_PCIE_MISC_PCIE_STATUS, "pi4-pcie-status")?;
    let status_reg = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_PCIE_STATUS)?;
    mask_and_clear_pcie_irq_sources(status_page);

    let config_page =
        map_pcie_reg_page_cached(hal, BCM2711_PCIE_EXT_CFG_DATA, "pi4-pcie-ext-data")?;
    let index_page =
        map_pcie_reg_page_cached(hal, BCM2711_PCIE_EXT_CFG_INDEX, "pi4-pcie-ext-index")?;
    let config_virt = same_page_reg_virt(config_page, BCM2711_PCIE_EXT_CFG_DATA)?;
    let index_reg = same_page_reg_virt(index_page, BCM2711_PCIE_EXT_CFG_INDEX)?;

    let status = ensure_pi4_pcie_root_ready(hal, status_page, status_reg, phase)?;
    let status_ready = pcie_status_link_up_and_rc(status);
    if !status_ready {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie status inconclusive status=0x{status:08x} action=defer-ext-cfg-proof reason=link-or-rc-not-ready"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(HalError::Unsupported("pcie-link-or-rc-not-ready"));
    }
    if phase == Pi4PcieProofPhase::PostMailboxReset
        && post_mailbox_ext_cfg_data_read_deferred(status)
    {
        let mut skipped = heapless::String::<240>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut skipped,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie ext-cfg skipped status=0x{status:08x} reason=link-not-active exact=deferred stage={}",
                phase.label()
            ),
        );
        boot_log::force_uart_line(skipped.as_str());
        return Err(HalError::Unsupported("pcie-link-not-active"));
    }

    configure_pi4_pcie_root_bridge(root_cfg_page)?;

    let mut select = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut select,
        format_args!(
            "[local-seat] vl805 bcm2711-pcie ext-cfg mapped source=hal status=0x{status:08x} action=select-device"
        ),
    );
    boot_log::force_uart_line(select.as_str());

    let mut vendor_id = 0xffff;
    let mut device_id = 0xffff;
    let mut vendor_device = 0xffff_ffff;
    for attempt in 0..=PCIE_EXT_CFG_SELECTOR_RETRIES {
        vendor_id = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_VENDOR_DEVICE)?;
        device_id = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_VENDOR_DEVICE + 2)?;
        vendor_device = vl805_vendor_device_dword(vendor_id, device_id);
        if !vl805_ext_cfg_selector_echo(vendor_device) {
            break;
        }
        if attempt < PCIE_EXT_CFG_SELECTOR_RETRIES {
            pcie_spin_delay(PCIE_EXT_CFG_SELECT_SETTLE_SPINS);
        }
    }
    if vendor_id != VL805_PCI_VENDOR_ID || device_id != VL805_PCI_DEVICE_ID {
        let selector_echo = vl805_ext_cfg_selector_echo(vendor_device);
        let mut line = heapless::String::<208>::new();
        if selector_echo {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie id mismatch got={vendor_id:04x}:{device_id:04x} reason=selector-echo idx=0x{VL805_PCI_DEV_ADDR:08x}"
                ),
            );
        } else {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie id mismatch got={vendor_id:04x}:{device_id:04x}"
                ),
            );
        }
        boot_log::force_uart_line(line.as_str());
        if !status_ready {
            let mut skipped = heapless::String::<224>::new();
            if selector_echo {
                let _ = core::fmt::Write::write_fmt(
                    &mut skipped,
                    format_args!(
                        "[local-seat] vl805 bcm2711-pcie ext-cfg skipped status=0x{status:08x} reason=link-or-rc-not-ready exact=selector-echo"
                    ),
                );
            } else {
                let _ = core::fmt::Write::write_fmt(
                    &mut skipped,
                    format_args!(
                        "[local-seat] vl805 bcm2711-pcie ext-cfg skipped status=0x{status:08x} reason=link-or-rc-not-ready exact=id"
                    ),
                );
            }
            boot_log::force_uart_line(skipped.as_str());
            return Err(HalError::Unsupported("pcie-link-or-rc-not-ready"));
        }
        return Err(HalError::Unsupported("vl805-id"));
    }

    let class_revision = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_CLASS_REVISION)?;
    let class_code = (class_revision >> 8) & 0x00ff_ffff;
    if class_code != VL805_EXPECTED_CLASS_CODE {
        let mut line = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie class mismatch got=0x{class_code:06x} expected=0x{VL805_EXPECTED_CLASS_CODE:06x}"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        if !status_ready {
            let mut skipped = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut skipped,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie ext-cfg skipped status=0x{status:08x} reason=link-or-rc-not-ready exact=class"
                ),
            );
            boot_log::force_uart_line(skipped.as_str());
            return Err(HalError::Unsupported("pcie-link-or-rc-not-ready"));
        }
        return Err(HalError::Unsupported("vl805-class"));
    }

    let command_before = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS)?;
    let mut bar0 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR0)?;
    let mut bar1 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR1)?;
    let bar0_before = bar0;
    let bar1_before = bar1;
    if status_ready && vl805_bar_assignment_needed(bar0, bar1) {
        let assigned_bar0 = vl805_pi4_assigned_bar0_value();
        vl805_cfg_write_u32(index_reg, config_virt, PCI_CFG_BAR1, 0)?;
        vl805_cfg_write_u32(index_reg, config_virt, PCI_CFG_BAR0, assigned_bar0)?;
        fence(Ordering::SeqCst);
        pcie_spin_delay(PCIE_EXT_CFG_SELECT_SETTLE_SPINS);
        let reassigned_bar0 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR0)?;
        let reassigned_bar1 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR1)?;
        let mut line = heapless::String::<240>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie bar assign old=0x{bar0:08x}/0x{bar1:08x} new=0x{reassigned_bar0:08x}/0x{reassigned_bar1:08x} reason=unassigned-64bit-memory-bar"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        bar0 = reassigned_bar0;
        bar1 = reassigned_bar1;
    }
    let Some(mmio) = translate_vl805_pci_bar_to_cpu_mmio(bar0, bar1) else {
        let mut line = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie reject bar=0x{bar0:08x}/0x{bar1:08x} reason=no-mmio-bar"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        if !status_ready {
            let mut skipped = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut skipped,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie ext-cfg skipped status=0x{status:08x} reason=link-or-rc-not-ready exact=bar"
                ),
            );
            boot_log::force_uart_line(skipped.as_str());
            return Err(HalError::Unsupported("pcie-link-or-rc-not-ready"));
        }
        return Err(HalError::Unsupported("vl805-bar"));
    };
    if mmio != RPI4_VL805_XHCI_MMIO {
        let mut line = heapless::String::<240>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie reject bar=0x{bar0:08x}/0x{bar1:08x} translated=0x{mmio:016x} reason=unexpected-bar"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        if !status_ready {
            let mut skipped = heapless::String::<240>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut skipped,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie ext-cfg skipped status=0x{status:08x} reason=link-or-rc-not-ready exact=bar translated=0x{mmio:016x}"
                ),
            );
            boot_log::force_uart_line(skipped.as_str());
            return Err(HalError::Unsupported("pcie-link-or-rc-not-ready"));
        }
        return Err(HalError::Unsupported("vl805-bar"));
    }

    let command_masked = vl805_poll_only_intx_mask_command(command_before);
    if command_masked != command_before {
        vl805_cfg_write_u16(
            index_reg,
            config_virt,
            PCI_CFG_COMMAND_STATUS,
            command_masked,
        )?;
    }
    let command_masked_after = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS)?;
    if (command_masked_after & PCI_COMMAND_INTERRUPT_DISABLE) == 0 {
        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie reject cmd=0x{command_before:04x}->0x{command_masked_after:04x} reason=intx-mask-not-ready"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(HalError::Unsupported("vl805-intx-mask"));
    }

    let interrupt_proof = quiesce_vl805_interrupt_modes_for_poll_only(index_reg, config_virt)?;
    let devctl_proof = configure_vl805_pcie_device_control(index_reg, config_virt)?;

    let command_required = vl805_poll_only_bus_master_command(command_masked_after);
    if command_required != command_masked_after {
        vl805_cfg_write_u16(
            index_reg,
            config_virt,
            PCI_CFG_COMMAND_STATUS,
            command_required,
        )?;
    }
    let command_after = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS)?;
    if !vl805_command_ownership_ready(command_after) {
        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie reject cmd=0x{command_before:04x}->0x{command_after:04x} reason=command-not-ready"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(HalError::Unsupported("vl805-command"));
    }

    if vl805_post_command_reset_notify_needed(
        phase,
        bar0_before,
        bar1_before,
        bar0,
        bar1,
        command_before,
        command_after,
        devctl_proof,
    ) {
        notify_vl805_reset_after_pcie_ready(
            hal,
            phase,
            "post-vl805-bar-command",
            "vl805-bar-command-devctl-after-firmware-notify",
        )?;
    }

    Ok(Pi4Vl805PcieProof {
        status,
        config_virt,
        vendor_id,
        device_id,
        class_code,
        command_before,
        command_after,
        bar0,
        bar1,
        mmio,
        msi_control_before: interrupt_proof.msi_control_before,
        msi_control_after: interrupt_proof.msi_control_after,
        msix_control_before: interrupt_proof.msix_control_before,
        msix_control_after: interrupt_proof.msix_control_after,
        pcie_devctl_before: devctl_proof.control_before,
        pcie_devctl_after: devctl_proof.control_after,
    })
}

fn vl805_post_command_reset_notify_needed(
    phase: Pi4PcieProofPhase,
    bar0_before: u32,
    bar1_before: u32,
    bar0_after: u32,
    bar1_after: u32,
    command_before: u16,
    command_after: u16,
    devctl_proof: Vl805PcieDeviceControlProof,
) -> bool {
    if !phase.reloads_vl805_firmware_after_perst() {
        return false;
    }
    let bar_changed = bar0_before != bar0_after || bar1_before != bar1_after;
    let command_changed = command_before != command_after;
    let devctl_changed = match (devctl_proof.control_before, devctl_proof.control_after) {
        (Some(before), Some(after)) => before != after,
        _ => false,
    };
    bar_changed || command_changed || devctl_changed
}

fn ensure_pi4_pcie_root_ready(
    hal: &mut KernelHal<'_>,
    status_page: usize,
    status_reg: usize,
    phase: Pi4PcieProofPhase,
) -> Result<u32, HalError> {
    let status_before = mmio_read_u32(status_reg);
    let misc_ctrl = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_MISC_CTRL)?;
    let rc_bar1 = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR1_CONFIG_LO)?;
    let rc_bar2_lo = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR2_CONFIG_LO)?;
    let rc_bar2_hi = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR2_CONFIG_HI)?;
    let rc_bar3 = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR3_CONFIG_LO)?;
    if pcie_root_ready_fast_path_allowed(phase, status_before) {
        remember_pi4_pcie_link_and_rc_ready(status_before);
        mmio_clear_set_bits_u32_flush(
            misc_ctrl,
            PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_MASK,
            PCIE_MISC_MISC_CTRL_SCB_ACCESS_EN_MASK
                | PCIE_MISC_MISC_CTRL_CFG_READ_UR_MODE_MASK
                | PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_128,
        );
        configure_pi4_pcie_dma_window(misc_ctrl, rc_bar1, rc_bar2_lo, rc_bar2_hi, rc_bar3);
        mask_and_clear_pcie_irq_sources(status_page);
        configure_pi4_pcie_outbound_window(status_page)?;
        notify_vl805_reset_after_pcie_ready(
            hal,
            phase,
            "post-mailbox-pcie-ready",
            "pcie-ready-after-firmware-notify",
        )?;
        let mut line = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie root-init ready stage={} status=0x{status_before:08x} action=refresh-windows-vl805-notify source=hal",
                phase.label()
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Ok(status_before);
    }

    if phase
        .root_init_latch()
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie root-init skipped stage={} status=0x{status_before:08x} reason=already-attempted",
                phase.label()
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Ok(status_before);
    }

    let init_page = map_pcie_reg_page_cached(hal, BCM2711_PCIE_RGR1_SW_INIT_1, "pi4-pcie-sw-init")?;
    let sw_init_reg = same_page_reg_virt(init_page, BCM2711_PCIE_RGR1_SW_INIT_1)?;
    let hard_debug = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_HARD_PCIE_HARD_DEBUG)?;

    let mut begin = heapless::String::<208>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut begin,
        format_args!(
            "[local-seat] vl805 bcm2711-pcie root-init begin stage={} status_before=0x{status_before:08x} source=hal",
            phase.label()
        ),
    );
    boot_log::force_uart_line(begin.as_str());

    mmio_set_bits_u32_flush(
        sw_init_reg,
        PCIE_RGR1_SW_INIT_1_INIT_MASK | PCIE_RGR1_SW_INIT_1_PERST_MASK,
    );
    pcie_spin_delay(PCIE_SHORT_SETTLE_SPINS);

    mmio_clear_bits_u32_flush(sw_init_reg, PCIE_RGR1_SW_INIT_1_INIT_MASK);
    mmio_clear_bits_u32_flush(hard_debug, PCIE_HARD_DEBUG_SERDES_IDDQ_MASK);
    pcie_spin_delay(PCIE_SHORT_SETTLE_SPINS);

    mmio_clear_set_bits_u32_flush(
        misc_ctrl,
        PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_MASK,
        PCIE_MISC_MISC_CTRL_SCB_ACCESS_EN_MASK
            | PCIE_MISC_MISC_CTRL_CFG_READ_UR_MODE_MASK
            | PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_128,
    );
    configure_pi4_pcie_dma_window(misc_ctrl, rc_bar1, rc_bar2_lo, rc_bar2_hi, rc_bar3);
    mask_and_clear_pcie_irq_sources(status_page);

    mmio_clear_bits_u32_flush(sw_init_reg, PCIE_RGR1_SW_INIT_1_PERST_MASK);
    fence(Ordering::SeqCst);
    pcie_spin_delay(PCIE_POST_PERST_SETTLE_SPINS);

    let mut status_after = mmio_read_u32(status_reg);
    let mut polls = 0usize;
    while polls < PCIE_LINK_POLL_ATTEMPTS && !pcie_status_link_up_and_rc(status_after) {
        pcie_spin_delay(PCIE_LINK_POLL_SPINS);
        polls += 1;
        status_after = mmio_read_u32(status_reg);
    }

    let ready = remember_pi4_pcie_link_and_rc_ready(status_after);
    if ready {
        if let Err(err) = configure_pi4_pcie_outbound_window(status_page) {
            finish_pi4_pcie_root_init_attempt(phase, false);
            return Err(err);
        }
        notify_vl805_reset_after_pcie_ready(
            hal,
            phase,
            "post-pcie-perst",
            "pcie-perst-after-firmware-notify",
        )?;
    } else {
        finish_pi4_pcie_root_init_attempt(phase, false);
    }

    let mut done = heapless::String::<320>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut done,
        format_args!(
            "[local-seat] vl805 bcm2711-pcie root-init done stage={} status_before=0x{status_before:08x} status_after=0x{status_after:08x} ready={} polls={polls} post_perst_ms={} poll_window_ms={} poll_interval_ms={} delay_scale={} write_flush=readback retry={}",
            phase.label(),
            ready as u8,
            PCIE_POST_PERST_SETTLE_MS,
            PCIE_LINK_POLL_TOTAL_MS,
            PCIE_LINK_POLL_INTERVAL_MS,
            PCIE_ROOT_DELAY_SPIN_SAFETY_MULTIPLIER,
            if ready { "closed" } else { "armed" },
        ),
    );
    boot_log::force_uart_line(done.as_str());

    Ok(status_after)
}

fn remember_pi4_pcie_link_and_rc_ready(status: u32) -> bool {
    let ready = pcie_status_link_up_and_rc(status);
    if ready {
        PCIE_LINK_AND_RC_READY_PROVEN.store(1, Ordering::Release);
    }
    ready
}

fn configure_pi4_pcie_dma_window(
    misc_ctrl: usize,
    rc_bar1: usize,
    rc_bar2_lo: usize,
    rc_bar2_hi: usize,
    rc_bar3: usize,
) {
    let dma_offset = RPI4_PCIE_DMA_BUS_BASE.saturating_sub(RPI4_PCIE_DMA_CPU_BASE);
    let dma_size = pcie_next_power_of_two(RPI4_PCIE_DMA_WINDOW_BYTES);
    let rc_bar2_value = replace_u32_field(
        dma_offset as u32,
        PCIE_MISC_RC_BAR2_CONFIG_LO_SIZE_MASK,
        brcm_pcie_encode_ibar_size(dma_size),
    );
    mmio_write_u32_flush(rc_bar2_lo, rc_bar2_value);
    mmio_write_u32_flush(rc_bar2_hi, (dma_offset >> 32) as u32);

    let scb_size = pcie_log2_power_of_two(dma_size)
        .and_then(|log2| log2.checked_sub(15))
        .unwrap_or(0x0f);
    mmio_clear_set_bits_u32_flush(
        misc_ctrl,
        PCIE_MISC_MISC_CTRL_SCB0_SIZE_MASK,
        replace_u32_field(0, PCIE_MISC_MISC_CTRL_SCB0_SIZE_MASK, scb_size as u32),
    );
    mmio_clear_bits_u32_flush(rc_bar1, PCIE_MISC_RC_BAR1_CONFIG_LO_SIZE_MASK);
    mmio_clear_bits_u32_flush(rc_bar3, PCIE_MISC_RC_BAR3_CONFIG_LO_SIZE_MASK);

    let rc_bar2_lo_after = mmio_read_u32(rc_bar2_lo);
    let rc_bar2_hi_after = mmio_read_u32(rc_bar2_hi);
    let misc_ctrl_after = mmio_read_u32(misc_ctrl);
    let rc_bar1_after = mmio_read_u32(rc_bar1);
    let rc_bar3_after = mmio_read_u32(rc_bar3);
    let mut line = heapless::String::<320>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] vl805 bcm2711-pcie dma-window bus_base=0x{bus:016x} cpu_base=0x{cpu:016x} bytes=0x{bytes:016x} rc_bar2=0x{lo:08x}/0x{hi:08x} misc_ctrl=0x{misc:08x} scb_size={scb_size} rc_bar1=0x{bar1:08x} rc_bar3=0x{bar3:08x} source=hal",
            bus = RPI4_PCIE_DMA_BUS_BASE,
            cpu = RPI4_PCIE_DMA_CPU_BASE,
            bytes = RPI4_PCIE_DMA_WINDOW_BYTES,
            lo = rc_bar2_lo_after,
            hi = rc_bar2_hi_after,
            misc = misc_ctrl_after,
            bar1 = rc_bar1_after,
            bar3 = rc_bar3_after,
        ),
    );
    boot_log::force_uart_line(line.as_str());
}

fn configure_pi4_pcie_outbound_window(status_page: usize) -> Result<(), HalError> {
    let win_lo = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LO)?;
    let win_hi = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_HI)?;
    let base_limit = same_page_reg_virt(
        status_page,
        BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT,
    )?;
    let base_hi = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_HI)?;
    let limit_hi = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LIMIT_HI)?;
    configure_pcie_outbound_window_regs(
        win_lo,
        win_hi,
        base_limit,
        base_hi,
        limit_hi,
        RPI4_PCIE_CPU_MMIO_WINDOW_BASE as u64,
        RPI4_PCIE_BUS_MMIO_WINDOW_BASE as u64,
        RPI4_PCIE_MMIO_WINDOW_SIZE,
    );
    Ok(())
}

fn configure_pi4_pcie_root_bridge(root_cfg_page: usize) -> Result<(), HalError> {
    let endian_reg = same_page_reg_virt(
        root_cfg_page,
        BCM2711_PCIE_RC_CFG_VENDOR_VENDOR_SPECIFIC_REG1,
    )?;
    let class_reg = same_page_reg_virt(root_cfg_page, BCM2711_PCIE_RC_CFG_PRIV1_ID_VAL3)?;
    let link_cap_reg =
        same_page_reg_virt(root_cfg_page, BCM2711_PCIE_RC_CFG_PRIV1_LINK_CAPABILITY)?;
    let vendor_device = pci_cfg_read_u32(root_cfg_page, PCI_CFG_VENDOR_DEVICE);
    let vendor_id = (vendor_device & 0xffff) as u16;
    let device_id = (vendor_device >> 16) as u16;
    let class_before = pci_cfg_read_u32(root_cfg_page, PCI_CFG_CLASS_REVISION) >> 8;
    let endian_before = mmio_read_u32(endian_reg);
    let link_cap_before = mmio_read_u32(link_cap_reg);

    mmio_clear_set_bits_u32_flush(
        class_reg,
        PCIE_RC_CFG_PRIV1_ID_VAL3_CLASS_CODE_MASK,
        BCM2711_ROOT_BRIDGE_CLASS_CODE,
    );
    pci_cfg_write_u32(
        root_cfg_page,
        PCI_CFG_PRIMARY_BUS,
        RPI4_PCIE_BRIDGE_BUS_NUMBERS,
    );
    pci_cfg_write_u32(
        root_cfg_page,
        PCI_CFG_IO_BASE_LIMIT,
        RPI4_PCIE_BRIDGE_IO_BASE_LIMIT_DISABLED,
    );
    pci_cfg_write_u32(
        root_cfg_page,
        PCI_CFG_MEMORY_BASE_LIMIT,
        rpi4_vl805_bridge_memory_base_limit(),
    );
    pci_cfg_write_u32(
        root_cfg_page,
        PCI_CFG_PREFETCH_BASE_LIMIT,
        RPI4_PCIE_BRIDGE_PREFETCH_BASE_LIMIT_DISABLED,
    );

    let command_before = pci_cfg_read_u16(root_cfg_page, PCI_CFG_COMMAND_STATUS);
    let command_required = command_before | RPI4_PCIE_BRIDGE_COMMAND_REQUIRED;
    if command_required != command_before {
        pci_cfg_write_u16(root_cfg_page, PCI_CFG_COMMAND_STATUS, command_required);
    }
    let endian_after = mmio_clear_bits_u32_flush(
        endian_reg,
        PCIE_RC_CFG_VENDOR_SPECIFIC_REG1_ENDIAN_MODE_BAR2_MASK,
    );
    let link_cap_after = mmio_clear_bits_u32_flush(
        link_cap_reg,
        PCIE_RC_CFG_PRIV1_LINK_CAPABILITY_ASPM_SUPPORT_MASK,
    );
    fence(Ordering::SeqCst);
    pcie_spin_delay(PCIE_EXT_CFG_SELECT_SETTLE_SPINS);

    let command_after = pci_cfg_read_u16(root_cfg_page, PCI_CFG_COMMAND_STATUS);
    let bus_after = pci_cfg_read_u32(root_cfg_page, PCI_CFG_PRIMARY_BUS);
    let mem_after = pci_cfg_read_u32(root_cfg_page, PCI_CFG_MEMORY_BASE_LIMIT);
    let pref_after = pci_cfg_read_u32(root_cfg_page, PCI_CFG_PREFETCH_BASE_LIMIT);
    let class_after = pci_cfg_read_u32(root_cfg_page, PCI_CFG_CLASS_REVISION) >> 8;

    let mut line = heapless::String::<384>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] vl805 bcm2711-pcie bridge cfg vid:did={vendor_id:04x}:{device_id:04x} class=0x{class_before:06x}->0x{class_after:06x} bus=0x{bus_after:08x} mem=0x{mem_after:08x} prefetch=0x{pref_after:08x} cmd=0x{command_before:04x}->0x{command_after:04x} bar2_endian=0x{endian_before:08x}->0x{endian_after:08x} aspm=0x{link_cap_before:08x}->0x{link_cap_after:08x} source=hal-root-port"
        ),
    );
    boot_log::force_uart_line(line.as_str());

    if vendor_id != BCM2711_ROOT_VENDOR_ID || device_id != BCM2711_ROOT_DEVICE_ID {
        return Err(HalError::Unsupported("pcie-root-id"));
    }
    if bus_after != RPI4_PCIE_BRIDGE_BUS_NUMBERS {
        return Err(HalError::Unsupported("pcie-root-bus-window"));
    }
    if mem_after != rpi4_vl805_bridge_memory_base_limit() {
        return Err(HalError::Unsupported("pcie-root-mem-window"));
    }
    if (command_after & RPI4_PCIE_BRIDGE_COMMAND_REQUIRED) != RPI4_PCIE_BRIDGE_COMMAND_REQUIRED {
        return Err(HalError::Unsupported("pcie-root-command"));
    }
    if (endian_after & PCIE_RC_CFG_VENDOR_SPECIFIC_REG1_ENDIAN_MODE_BAR2_MASK) != 0 {
        return Err(HalError::Unsupported("pcie-root-bar2-endian"));
    }
    if (link_cap_after & PCIE_RC_CFG_PRIV1_LINK_CAPABILITY_ASPM_SUPPORT_MASK) != 0 {
        return Err(HalError::Unsupported("pcie-root-aspm"));
    }
    Ok(())
}

fn configure_pcie_outbound_window_regs(
    win_lo: usize,
    win_hi: usize,
    base_limit: usize,
    base_hi: usize,
    limit_hi: usize,
    cpu_addr: u64,
    pcie_addr: u64,
    size: u64,
) {
    mmio_write_u32_flush(win_lo, pcie_addr as u32);
    mmio_write_u32_flush(win_hi, (pcie_addr >> 32) as u32);

    mmio_clear_set_bits_u32_flush(
        base_limit,
        PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT_BASE_MASK
            | PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT_LIMIT_MASK,
        pcie_outbound_base_limit_value(cpu_addr, size),
    );
    mmio_clear_set_bits_u32_flush(
        base_hi,
        PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_HI_BASE_MASK,
        pcie_outbound_base_hi_value(cpu_addr),
    );
    mmio_clear_set_bits_u32_flush(
        limit_hi,
        PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LIMIT_HI_LIMIT_MASK,
        pcie_outbound_limit_hi_value(cpu_addr, size),
    );
}

fn pcie_spin_delay(spins: usize) {
    pcie_spin_delay_with(
        spins,
        |count| {
            for _ in 0..count {
                core::hint::spin_loop();
            }
        },
        || {
            // HAL's flushed MMIO waits hold no driver Reply or frame ticket.
            // This only offers the existing child-owned startup tile a turn;
            // its CNTVCT guard disables the path after serial cutover.
            let _ = super::poll_early_hdmi_boot_progress();
        },
    );
}

fn pcie_spin_delay_with(
    mut remaining: usize,
    mut delay: impl FnMut(usize),
    mut checkpoint: impl FnMut(),
) {
    // Preserve every original hardware-settle iteration. Chunking provides
    // service opportunities; it is neither a clock nor a shorter PCIe wait.
    while remaining != 0 {
        let count = remaining.min(50_000);
        delay(count);
        remaining -= count;
        checkpoint();
    }
}

#[inline]
fn mmio_write_u32_flush(addr: usize, value: u32) -> u32 {
    mmio_write_u32(addr, value);
    fence(Ordering::SeqCst);
    let observed = mmio_read_u32(addr);
    fence(Ordering::SeqCst);
    observed
}

#[inline]
fn mmio_set_bits_u32_flush(addr: usize, bits: u32) -> u32 {
    let value = mmio_read_u32(addr);
    mmio_write_u32_flush(addr, value | bits)
}

#[inline]
fn mmio_clear_bits_u32_flush(addr: usize, bits: u32) -> u32 {
    let value = mmio_read_u32(addr);
    mmio_write_u32_flush(addr, value & !bits)
}

#[inline]
fn mmio_clear_set_bits_u32_flush(addr: usize, clear: u32, set: u32) -> u32 {
    let value = mmio_read_u32(addr);
    mmio_write_u32_flush(addr, (value & !clear) | set)
}

#[inline]
const fn pcie_outbound_base_limit_value(cpu_addr: u64, size: u64) -> u32 {
    let base_mb = cpu_addr / 0x10_0000;
    let limit_mb = cpu_addr.saturating_add(size).saturating_sub(1) / 0x10_0000;
    replace_u32_field(
        replace_u32_field(
            0,
            PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT_BASE_MASK,
            base_mb as u32,
        ),
        PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT_LIMIT_MASK,
        limit_mb as u32,
    )
}

#[inline]
const fn pcie_outbound_base_hi_value(cpu_addr: u64) -> u32 {
    let base_mb = cpu_addr / 0x10_0000;
    (base_mb >> 12) as u32 & PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_HI_BASE_MASK
}

#[inline]
const fn pcie_outbound_limit_hi_value(cpu_addr: u64, size: u64) -> u32 {
    let limit_mb = cpu_addr.saturating_add(size).saturating_sub(1) / 0x10_0000;
    (limit_mb >> 12) as u32 & PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LIMIT_HI_LIMIT_MASK
}

#[inline]
const fn pci_bridge_memory_base_limit(bus_addr: u32, size: u32) -> u32 {
    let base = (bus_addr >> 16) & 0xfff0;
    let limit = (bus_addr.saturating_add(size).saturating_sub(1) >> 16) & 0xfff0;
    (limit << 16) | base
}

#[inline]
const fn rpi4_vl805_bridge_memory_base_limit() -> u32 {
    pci_bridge_memory_base_limit(
        RPI4_PCIE_BUS_MMIO_WINDOW_BASE_U32,
        RPI4_VL805_BRIDGE_MMIO_WINDOW_BYTES,
    )
}

#[inline]
const fn replace_u32_field(raw: u32, mask: u32, value: u32) -> u32 {
    if mask == 0 {
        return raw;
    }
    let shift = mask.trailing_zeros();
    (raw & !mask) | ((value << shift) & mask)
}

const fn brcm_pcie_encode_ibar_size(size: u64) -> u32 {
    match pcie_log2_power_of_two(size) {
        Some(log2) if log2 >= 12 && log2 <= 15 => (log2 - 12) as u32 + 0x1c,
        Some(log2) if log2 >= 16 && log2 <= 37 => (log2 - 15) as u32,
        _ => 0,
    }
}

const fn pcie_next_power_of_two(value: u64) -> u64 {
    if value <= 1 {
        return 1;
    }
    match value.checked_next_power_of_two() {
        Some(power) => power,
        None => u64::MAX,
    }
}

const fn pcie_log2_power_of_two(value: u64) -> Option<u32> {
    if value == 0 || (value & (value - 1)) != 0 {
        return None;
    }
    Some(63 - value.leading_zeros())
}

fn map_pcie_reg_page_cached(
    hal: &mut KernelHal<'_>,
    reg_offset: usize,
    label: &'static str,
) -> Result<usize, HalError> {
    let cache = match reg_offset & !PAGE_MASK {
        page if page == (PCI_CFG_VENDOR_DEVICE & !PAGE_MASK) => &PCIE_ROOT_CFG_PAGE_VIRT,
        page if page == (BCM2711_PCIE_MISC_PCIE_STATUS & !PAGE_MASK) => &PCIE_STATUS_PAGE_VIRT,
        page if page == (BCM2711_PCIE_EXT_CFG_DATA & !PAGE_MASK) => &PCIE_EXT_DATA_PAGE_VIRT,
        page if page == (BCM2711_PCIE_EXT_CFG_INDEX & !PAGE_MASK) => &PCIE_EXT_INDEX_PAGE_VIRT,
        _ => return Err(HalError::Unsupported("pcie-reg-page")),
    };
    let cached = cache.load(Ordering::Acquire);
    if cached != 0 {
        return Ok(cached);
    }

    let (page_paddr, _) = pcie_reg_page(reg_offset)?;
    let mut prefix_maps = Vec::new();
    let frame = map_device_exact(hal, page_paddr, label, &mut prefix_maps)?;
    let page_virt = frame.ptr().as_ptr() as usize;
    let stored = cache
        .compare_exchange(0, page_virt, Ordering::AcqRel, Ordering::Acquire)
        .unwrap_or_else(|stored| stored);
    if stored == page_virt {
        core::mem::forget(frame);
    }
    Ok(stored)
}

fn map_device_exact(
    hal: &mut KernelHal<'_>,
    paddr: usize,
    label: &'static str,
    prefix_maps: &mut Vec<crate::sel4::DeviceFrame>,
) -> Result<crate::sel4::DeviceFrame, HalError> {
    let coverage = hal.device_coverage(paddr, PAGE_BITS);
    if coverage.is_none() {
        if let Ok(frame) = hal.map_device(paddr) {
            let actual_paddr = page_get_address(frame.cap()).map_err(HalError::from)?;
            if actual_paddr == paddr {
                let mut line = heapless::String::<192>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] {label} map exact reuse paddr=0x{paddr:016x} reason=device-frame-cache"
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return Ok(frame);
            }
        }
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] {label} map exact miss paddr=0x{paddr:016x} reason=no-device-coverage"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(HalError::Unsupported("device-coverage"));
    }
    let Some(coverage) = coverage else {
        return Err(HalError::Unsupported("device-coverage"));
    };
    let span_bytes = coverage.limit.saturating_sub(coverage.base);
    let span_pages = cmp::max(1usize, div_ceil(span_bytes, PAGE_SIZE));
    let max_attempts = cmp::max(
        1usize,
        cmp::min(span_pages.saturating_add(1), MAP_EXACT_ATTEMPT_CAP),
    );

    for attempt in 0..max_attempts {
        let frame = hal.map_device(paddr)?;
        let actual_paddr = page_get_address(frame.cap()).map_err(HalError::from)?;
        if actual_paddr == paddr {
            if attempt > 0 {
                let mut line = heapless::String::<208>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] {label} map exact after retries={} base=0x{:08x} limit=0x{:08x}",
                        attempt, coverage.base, coverage.limit
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            return Ok(frame);
        }

        if should_log_exact_map_retry(attempt, max_attempts) {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] {label} map mismatch want=0x{paddr:08x} got=0x{actual_paddr:08x} attempt={}/{}",
                    attempt + 1,
                    max_attempts
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }

        if actual_paddr > paddr {
            return Err(HalError::Unsupported("device-map-order"));
        }
        prefix_maps.push(frame);
    }

    Err(HalError::Unsupported("device-map-exact"))
}

const fn should_log_exact_map_retry(attempt: usize, max_attempts: usize) -> bool {
    attempt < 4 || attempt + 1 == max_attempts || (attempt + 1) % EXACT_MAP_LOG_STRIDE == 0
}

fn same_page_reg_virt(page_virt: usize, reg_offset: usize) -> Result<usize, HalError> {
    page_virt
        .checked_add(reg_offset & PAGE_MASK)
        .ok_or(HalError::Unsupported("pcie-reg-virt"))
}

fn pcie_reg_page(offset: usize) -> Result<(usize, usize), HalError> {
    let paddr = BCM2711_PCIE_HOST_PHYS_BASE
        .checked_add(offset)
        .ok_or(HalError::Unsupported("pcie-reg-paddr"))?;
    Ok((paddr & !PAGE_MASK, paddr & PAGE_MASK))
}

fn mask_and_clear_pcie_irq_sources(status_page_virt: usize) {
    if let (Ok(cpu_mask_set), Ok(cpu_clr), Ok(msi_mask_set), Ok(msi_clr)) = (
        same_page_reg_virt(status_page_virt, BCM2711_PCIE_INTR2_CPU_MASK_SET),
        same_page_reg_virt(status_page_virt, BCM2711_PCIE_INTR2_CPU_CLR),
        same_page_reg_virt(status_page_virt, BCM2711_PCIE_MSI_INTR2_MASK_SET),
        same_page_reg_virt(status_page_virt, BCM2711_PCIE_MSI_INTR2_CLR),
    ) {
        let cpu_mask = mmio_write_u32_flush(cpu_mask_set, u32::MAX);
        let cpu_clear = mmio_write_u32_flush(cpu_clr, u32::MAX);
        let msi_mask = mmio_write_u32_flush(msi_mask_set, u32::MAX);
        let msi_clear = mmio_write_u32_flush(msi_clr, u32::MAX);
        let trusted_readback =
            pcie_irq_source_mask_readback_trusted(cpu_mask, cpu_clear, msi_mask, msi_clear);
        if trusted_readback {
            PCIE_IRQ_SOURCES_MASKED_PROVEN.store(1, Ordering::Release);
        }
        let mut line = heapless::String::<192>::new();
        if trusted_readback {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie irq sources masked proof=trusted source=hal-ext-cfg readback=0x{cpu_mask:08x}/0x{cpu_clear:08x}/0x{msi_mask:08x}/0x{msi_clear:08x}"
                ),
            );
        } else {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie irq sources masked proof=untrusted reason=sentinel-readback source=hal-ext-cfg readback=0x{cpu_mask:08x}/0x{cpu_clear:08x}/0x{msi_mask:08x}/0x{msi_clear:08x}"
                ),
            );
        }
        boot_log::force_uart_line(line.as_str());
    }
}

#[inline]
const fn pcie_irq_source_mask_readback_trusted(
    cpu_mask: u32,
    cpu_clear: u32,
    msi_mask: u32,
    msi_clear: u32,
) -> bool {
    cpu_mask != u32::MAX && cpu_clear != u32::MAX && msi_mask != u32::MAX && msi_clear != u32::MAX
}

#[inline]
const fn pcie_status_link_up_and_rc(status: u32) -> bool {
    (status & BCM2711_PCIE_STATUS_DL_ACTIVE) != 0
        && (status & BCM2711_PCIE_STATUS_PHY_LINK_UP) != 0
        && (status & BCM2711_PCIE_STATUS_PORT) != 0
}

#[inline]
const fn post_mailbox_ext_cfg_data_read_deferred(status: u32) -> bool {
    !pcie_status_link_up_and_rc(status)
}

fn quiesce_vl805_interrupt_modes_for_poll_only(
    index_reg: usize,
    config_virt: usize,
) -> Result<Vl805InterruptModeProof, HalError> {
    let status = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS + 2)?;
    let mut proof = Vl805InterruptModeProof::default();
    if (status & PCI_STATUS_CAPABILITIES_LIST) == 0 {
        boot_log::force_uart_line(
            "[local-seat] vl805 bcm2711-pcie irq-mode proof skipped reason=no-cap-list",
        );
        return Ok(proof);
    }

    let mut saw_msi = false;
    let mut saw_msix = false;
    let mut cap =
        (vl805_cfg_read_u8(index_reg, config_virt, PCI_CFG_CAP_PTR)? & PCI_CAP_NEXT_MASK) as usize;
    for _ in 0..PCI_CAP_TRAVERSE_LIMIT {
        if !(0x40..0x100).contains(&cap) {
            break;
        }
        let cap_id = vl805_cfg_read_u8(index_reg, config_virt, cap)?;
        let next =
            (vl805_cfg_read_u8(index_reg, config_virt, cap + 1)? & PCI_CAP_NEXT_MASK) as usize;
        match cap_id {
            PCI_CAP_ID_MSI => {
                saw_msi = true;
                let ctrl_offset = cap + PCI_MSI_CONTROL_OFFSET;
                let control_before = vl805_cfg_read_u16(index_reg, config_virt, ctrl_offset)?;
                let control_request = vl805_msi_control_disable_value(control_before);
                if control_request != control_before {
                    vl805_cfg_write_u16(index_reg, config_virt, ctrl_offset, control_request)?;
                }
                let control_after = vl805_cfg_read_u16(index_reg, config_virt, ctrl_offset)?;
                let disabled = vl805_msi_control_disabled(control_after);
                let mut line = heapless::String::<240>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] vl805 bcm2711-pcie msi proof cap=0x{cap:02x} control=0x{control_before:04x}->0x{control_after:04x} disabled={}",
                        disabled as u8,
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                proof.msi_control_before = Some(control_before);
                proof.msi_control_after = Some(control_after);
                if !disabled {
                    return Err(HalError::Unsupported("vl805-msi"));
                }
            }
            PCI_CAP_ID_MSIX => {
                saw_msix = true;
                let ctrl_offset = cap + PCI_MSIX_CONTROL_OFFSET;
                let control_before = vl805_cfg_read_u16(index_reg, config_virt, ctrl_offset)?;
                let control_request = vl805_msix_control_quiesce_value(control_before);
                if control_request != control_before {
                    vl805_cfg_write_u16(index_reg, config_virt, ctrl_offset, control_request)?;
                }
                let control_after = vl805_cfg_read_u16(index_reg, config_virt, ctrl_offset)?;
                let disabled = vl805_msix_control_disabled(control_after);
                let maskall = (control_after & PCI_MSIX_CONTROL_MASKALL) != 0;
                let quiesced = vl805_msix_control_quiesced(control_after);
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] vl805 bcm2711-pcie msix proof cap=0x{cap:02x} control=0x{control_before:04x}->0x{control_after:04x} disabled={} maskall={} quiesced={}",
                        disabled as u8, maskall as u8, quiesced as u8,
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                proof.msix_control_before = Some(control_before);
                proof.msix_control_after = Some(control_after);
                if !quiesced {
                    return Err(HalError::Unsupported("vl805-msix"));
                }
            }
            _ => {}
        }
        if next == 0 || next == cap {
            break;
        }
        cap = next;
    }

    if !saw_msi {
        boot_log::force_uart_line(
            "[local-seat] vl805 bcm2711-pcie msi proof skipped reason=msi-cap-missing",
        );
    }
    if !saw_msix {
        boot_log::force_uart_line(
            "[local-seat] vl805 bcm2711-pcie msix proof skipped reason=msix-cap-missing",
        );
    }
    Ok(proof)
}

fn configure_vl805_pcie_device_control(
    index_reg: usize,
    config_virt: usize,
) -> Result<Vl805PcieDeviceControlProof, HalError> {
    let status = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS + 2)?;
    if (status & PCI_STATUS_CAPABILITIES_LIST) == 0 {
        boot_log::force_uart_line(
            "[local-seat] vl805 bcm2711-pcie devctl proof skipped reason=no-cap-list",
        );
        return Err(HalError::Unsupported("vl805-pcie-devctl"));
    }

    let mut cap =
        (vl805_cfg_read_u8(index_reg, config_virt, PCI_CFG_CAP_PTR)? & PCI_CAP_NEXT_MASK) as usize;
    for _ in 0..PCI_CAP_TRAVERSE_LIMIT {
        if !(0x40..0x100).contains(&cap) {
            break;
        }
        let cap_id = vl805_cfg_read_u8(index_reg, config_virt, cap)?;
        let next =
            (vl805_cfg_read_u8(index_reg, config_virt, cap + 1)? & PCI_CAP_NEXT_MASK) as usize;
        if cap_id == PCI_CAP_ID_EXP {
            let control_offset = cap + PCI_EXP_DEVCTL_OFFSET;
            let control_before = vl805_cfg_read_u16(index_reg, config_virt, control_offset)?;
            let control_request = vl805_pcie_devctl_command_proof_value(control_before);
            if control_request != control_before {
                vl805_cfg_write_u16(index_reg, config_virt, control_offset, control_request)?;
            }
            let control_after = vl805_cfg_read_u16(index_reg, config_virt, control_offset)?;
            let ready = vl805_pcie_devctl_ready(control_after);
            let mut line = heapless::String::<288>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 bcm2711-pcie devctl proof cap=0x{cap:02x} control=0x{control_before:04x}->0x{control_after:04x} target=0x{VL805_PCIE_DEVCTL_COMMAND_PROOF:04x} ready={} source=hal-ext-cfg policy=command-proof-linux-captured",
                    ready as u8,
                ),
            );
            boot_log::force_uart_line(line.as_str());
            if !ready {
                return Err(HalError::Unsupported("vl805-pcie-devctl"));
            }
            return Ok(Vl805PcieDeviceControlProof {
                control_before: Some(control_before),
                control_after: Some(control_after),
            });
        }
        if next == 0 || next == cap {
            break;
        }
        cap = next;
    }

    boot_log::force_uart_line(
        "[local-seat] vl805 bcm2711-pcie devctl proof skipped reason=pcie-cap-missing",
    );
    Err(HalError::Unsupported("vl805-pcie-devctl"))
}

#[inline]
const fn vl805_msi_control_disable_value(control: u16) -> u16 {
    control & !PCI_MSI_CONTROL_ENABLE
}

#[inline]
const fn vl805_msi_control_disabled(control: u16) -> bool {
    (control & PCI_MSI_CONTROL_ENABLE) == 0
}

#[inline]
const fn vl805_msix_control_quiesce_value(control: u16) -> u16 {
    (control | PCI_MSIX_CONTROL_MASKALL) & !PCI_MSIX_CONTROL_ENABLE
}

#[inline]
const fn vl805_msix_control_disabled(control: u16) -> bool {
    (control & PCI_MSIX_CONTROL_ENABLE) == 0
}

#[inline]
const fn vl805_msix_control_quiesced(control: u16) -> bool {
    vl805_msix_control_disabled(control) && (control & PCI_MSIX_CONTROL_MASKALL) != 0
}

#[inline]
const fn vl805_pcie_devctl_command_proof_value(_control: u16) -> u16 {
    VL805_PCIE_DEVCTL_COMMAND_PROOF
}

#[inline]
const fn vl805_pcie_devctl_ready(control: u16) -> bool {
    (control
        & (PCI_EXP_DEVCTL_CORR_ERR
            | PCI_EXP_DEVCTL_NON_FATAL_ERR
            | PCI_EXP_DEVCTL_FATAL_ERR
            | PCI_EXP_DEVCTL_UNSUP_REQ))
        == (VL805_PCIE_DEVCTL_COMMAND_PROOF
            & (PCI_EXP_DEVCTL_CORR_ERR
                | PCI_EXP_DEVCTL_NON_FATAL_ERR
                | PCI_EXP_DEVCTL_FATAL_ERR
                | PCI_EXP_DEVCTL_UNSUP_REQ))
        && (control & (PCI_EXP_DEVCTL_RELAXED_ORDERING | PCI_EXP_DEVCTL_NO_SNOOP))
            == (PCI_EXP_DEVCTL_RELAXED_ORDERING | PCI_EXP_DEVCTL_NO_SNOOP)
        && (control & PCI_EXP_DEVCTL_MAX_PAYLOAD_MASK) == PCI_EXP_DEVCTL_MAX_PAYLOAD_128B
        && (control & PCI_EXP_DEVCTL_MAX_READ_REQ_MASK) == PCI_EXP_DEVCTL_MAX_READ_REQ_512B
}

#[inline]
const fn vl805_poll_only_intx_mask_command(command: u16) -> u16 {
    command | PCI_COMMAND_INTERRUPT_DISABLE
}

#[inline]
const fn vl805_poll_only_bus_master_command(masked_command: u16) -> u16 {
    masked_command | VL805_POLL_ONLY_COMMAND_REQUIRED
}

#[inline]
const fn vl805_command_ownership_ready(command: u16) -> bool {
    (command & VL805_POLL_ONLY_COMMAND_REQUIRED) == VL805_POLL_ONLY_COMMAND_REQUIRED
}

fn translate_vl805_pci_bar_to_cpu_mmio(bar0: u32, bar1: u32) -> Option<usize> {
    let bus_mmio = decode_pci_mmio_bar(bar0, bar1)?;
    if bus_mmio == RPI4_VL805_XHCI_MMIO {
        return Some(bus_mmio);
    }
    let bus_offset = bus_mmio.checked_sub(RPI4_PCIE_BUS_MMIO_WINDOW_BASE)?;
    if bus_offset >= RPI4_PCIE_BUS_MMIO_WINDOW_BYTES {
        return None;
    }
    RPI4_PCIE_CPU_MMIO_WINDOW_BASE.checked_add(bus_offset)
}

fn vl805_exact_config_tuple_ready(
    vendor_device: u32,
    class_revision: u32,
    bar0: u32,
    bar1: u32,
) -> bool {
    (vendor_device & 0xffff) as u16 == VL805_PCI_VENDOR_ID
        && ((vendor_device >> 16) & 0xffff) as u16 == VL805_PCI_DEVICE_ID
        && ((class_revision >> 8) & 0x00ff_ffff) == VL805_EXPECTED_CLASS_CODE
        && translate_vl805_pci_bar_to_cpu_mmio(bar0, bar1) == Some(RPI4_VL805_XHCI_MMIO)
}

#[inline]
const fn vl805_bar_assignment_needed(bar0: u32, bar1: u32) -> bool {
    (bar0 & 0xf) == 0x4 && (bar0 & !0xf) == 0 && bar1 == 0
}

#[inline]
const fn vl805_pi4_assigned_bar0_value() -> u32 {
    RPI4_PCIE_BUS_MMIO_WINDOW_BASE_U32 | 0x4
}

#[inline]
const fn vl805_vendor_device_dword(vendor_id: u16, device_id: u16) -> u32 {
    vendor_id as u32 | ((device_id as u32) << 16)
}

#[inline]
const fn vl805_ext_cfg_selector_echo(value: u32) -> bool {
    value == VL805_PCI_DEV_ADDR
}

#[inline]
const fn vl805_ext_cfg_flush_read_valid(selected: u32, command_status: u32) -> bool {
    selected == VL805_PCI_DEV_ADDR
        && !vl805_ext_cfg_selector_echo(command_status)
        && command_status != 0xffff_ffff
}

fn decode_pci_mmio_bar(bar0: u32, bar1: u32) -> Option<usize> {
    if (bar0 & 0x1) != 0 {
        return None;
    }
    let is_64 = (bar0 & 0x6) == 0x4;
    let low = u64::from(bar0 & !0xf);
    let base = if is_64 {
        low | (u64::from(bar1) << 32)
    } else {
        low
    };
    usize::try_from(base).ok()
}

#[inline]
fn mmio_read_u32(addr: usize) -> u32 {
    // SAFETY: `addr` is derived from a HAL-mapped BCM2711 PCIe device page,
    // aligned by the register offset table, and volatile access is required
    // because the value is owned by hardware.
    unsafe { ptr::read_volatile(addr as *const u32) }
}

#[inline]
fn mmio_write_u32(addr: usize, value: u32) {
    // SAFETY: `addr` is derived from a HAL-mapped BCM2711 PCIe device page,
    // aligned by the register offset table, and volatile access is required
    // because the side effect is observed by hardware.
    unsafe {
        ptr::write_volatile(addr as *mut u32, value);
    }
}

#[inline]
fn bcm2711_ext_cfg_select(index_reg: usize) -> u32 {
    mmio_write_u32(index_reg, VL805_PCI_DEV_ADDR);
    fence(Ordering::SeqCst);
    let selected = mmio_read_u32(index_reg);
    fence(Ordering::SeqCst);
    pcie_spin_delay(PCIE_EXT_CFG_SELECT_SETTLE_SPINS);
    selected
}

#[inline]
fn vl805_cfg_select_or_err(
    index_reg: usize,
    offset: usize,
    op: &'static str,
) -> Result<(), HalError> {
    let selected = bcm2711_ext_cfg_select(index_reg);
    if selected == VL805_PCI_DEV_ADDR {
        return Ok(());
    }

    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] vl805 bcm2711-pcie ext-cfg reject op={op} offset=0x{offset:02x} selected=0x{selected:08x} expected=0x{VL805_PCI_DEV_ADDR:08x} reason=selector"
        ),
    );
    boot_log::force_uart_line(line.as_str());
    Err(HalError::Unsupported("vl805-ext-cfg-selector"))
}

#[inline]
fn vl805_cfg_read_u8(index_reg: usize, config_virt: usize, offset: usize) -> Result<u8, HalError> {
    vl805_cfg_select_or_err(index_reg, offset, "read8")?;
    Ok(pci_cfg_read_u8(config_virt, offset))
}

#[inline]
fn vl805_cfg_read_u16(
    index_reg: usize,
    config_virt: usize,
    offset: usize,
) -> Result<u16, HalError> {
    vl805_cfg_select_or_err(index_reg, offset, "read16")?;
    Ok(pci_cfg_read_u16(config_virt, offset))
}

#[inline]
fn vl805_cfg_write_u16(
    index_reg: usize,
    config_virt: usize,
    offset: usize,
    value: u16,
) -> Result<(), HalError> {
    vl805_cfg_select_or_err(index_reg, offset, "write16")?;
    pci_cfg_write_u16(config_virt, offset, value);
    Ok(())
}

#[inline]
fn vl805_cfg_write_u32(
    index_reg: usize,
    config_virt: usize,
    offset: usize,
    value: u32,
) -> Result<(), HalError> {
    vl805_cfg_select_or_err(index_reg, offset, "write32")?;
    pci_cfg_write_u32(config_virt, offset, value);
    Ok(())
}

#[inline]
fn vl805_cfg_read_u32(
    index_reg: usize,
    config_virt: usize,
    offset: usize,
) -> Result<u32, HalError> {
    vl805_cfg_select_or_err(index_reg, offset, "read32")?;
    Ok(pci_cfg_read_u32(config_virt, offset))
}

#[inline]
fn pci_cfg_read_u8(config_virt: usize, offset: usize) -> u8 {
    let Some(addr) = config_virt.checked_add(offset) else {
        return 0xff;
    };
    // SAFETY: `config_virt` is a HAL-owned BCM2711 PCIe config mapping.
    // PCI config byte reads are volatile MMIO.
    unsafe { ptr::read_volatile(addr as *const u8) }
}

#[inline]
fn pci_cfg_read_u16(config_virt: usize, offset: usize) -> u16 {
    let Some(addr) = config_virt.checked_add(offset) else {
        return 0xffff;
    };
    // SAFETY: `config_virt` is a HAL-owned BCM2711 PCIe config mapping.
    // PCI config word reads are volatile MMIO.
    unsafe { ptr::read_volatile(addr as *const u16) }
}

#[inline]
fn pci_cfg_write_u16(config_virt: usize, offset: usize, value: u16) {
    let Some(addr) = config_virt.checked_add(offset) else {
        return;
    };
    // SAFETY: `config_virt` is a HAL-owned BCM2711 PCIe config mapping.
    // PCI config word writes are volatile MMIO.
    unsafe {
        ptr::write_volatile(addr as *mut u16, value);
    }
}

#[inline]
fn pci_cfg_read_u32(config_virt: usize, offset: usize) -> u32 {
    let Some(addr) = config_virt.checked_add(offset) else {
        return 0xffff_ffff;
    };
    // SAFETY: `config_virt` is a HAL-owned BCM2711 PCIe config mapping.
    // PCI config dword reads are volatile MMIO.
    unsafe { ptr::read_volatile(addr as *const u32) }
}

#[inline]
fn pci_cfg_write_u32(config_virt: usize, offset: usize, value: u32) {
    let Some(addr) = config_virt.checked_add(offset) else {
        return;
    };
    // SAFETY: `config_virt` is a HAL-owned BCM2711 PCIe config mapping.
    // PCI config dword writes are volatile MMIO.
    unsafe {
        ptr::write_volatile(addr as *mut u32, value);
    }
}

const fn div_ceil(value: usize, divisor: usize) -> usize {
    if divisor == 0 {
        return value;
    }
    value.saturating_add(divisor - 1) / divisor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcie_status_requires_link_and_root_complex_mode() {
        assert!(pcie_status_link_up_and_rc(
            BCM2711_PCIE_STATUS_PORT
                | BCM2711_PCIE_STATUS_DL_ACTIVE
                | BCM2711_PCIE_STATUS_PHY_LINK_UP
        ));
        assert!(!pcie_status_link_up_and_rc(
            BCM2711_PCIE_STATUS_PORT | BCM2711_PCIE_STATUS_PHY_LINK_UP
        ));
        assert!(!pcie_status_link_up_and_rc(
            BCM2711_PCIE_STATUS_DL_ACTIVE | BCM2711_PCIE_STATUS_PHY_LINK_UP
        ));
        assert!(!pcie_status_link_up_and_rc(0xf2c0_000a));
    }

    #[test]
    fn pcie_progress_checkpoints_preserve_the_complete_settle_wait() {
        for (spins, expected_calls) in [(0, 0), (1, 1), (50_000, 1), (50_001, 2)] {
            let mut delayed = 0;
            let mut calls = 0;
            pcie_spin_delay_with(
                spins,
                |count| {
                    assert!((1..=50_000).contains(&count));
                    delayed += count;
                },
                || calls += 1,
            );
            assert_eq!(delayed, spins);
            assert_eq!(calls, expected_calls);
        }
    }

    #[test]
    fn bcm2711_root_init_timing_matches_uboot_link_contract() {
        assert_eq!(
            PCIE_SPINS_PER_MS,
            PCIE_ROOT_DELAY_BASE_SPINS_PER_MS * PCIE_ROOT_DELAY_SPIN_SAFETY_MULTIPLIER
        );
        assert!(PCIE_ROOT_DELAY_SPIN_SAFETY_MULTIPLIER >= 10);
        assert_eq!(PCIE_POST_PERST_SETTLE_MS, 100);
        assert_eq!(PCIE_LINK_POLL_TOTAL_MS, 100);
        assert_eq!(PCIE_LINK_POLL_INTERVAL_MS, 5);
        assert_eq!(PCIE_LINK_POLL_ATTEMPTS, 20);
        assert_eq!(PCIE_SHORT_SETTLE_SPINS, PCIE_SPINS_PER_MS / 10);
        assert!(PCIE_POST_PERST_SETTLE_SPINS >= 1_000 * PCIE_SHORT_SETTLE_SPINS);
        assert!(PCIE_EXT_CFG_SELECT_SETTLE_SPINS < PCIE_LINK_POLL_SPINS);
        assert_eq!(
            PCIE_LINK_POLL_ATTEMPTS * PCIE_LINK_POLL_INTERVAL_MS,
            PCIE_LINK_POLL_TOTAL_MS
        );
        assert_eq!(VL805_POST_PCIE_RESET_NOTIFY_SETTLE_MS, 20);
        assert_eq!(
            VL805_POST_PCIE_RESET_NOTIFY_SETTLE_SPINS,
            VL805_POST_PCIE_RESET_NOTIFY_SETTLE_MS * PCIE_SPINS_PER_MS
        );
    }

    #[test]
    fn post_mailbox_pcie_perst_reloads_vl805_firmware() {
        assert!(Pi4PcieProofPhase::Initial.powers_vl805_usb_hcd());
        assert!(!Pi4PcieProofPhase::PostMailboxReset.powers_vl805_usb_hcd());
        assert!(!Pi4PcieProofPhase::Initial.reloads_vl805_firmware_after_perst());
        assert!(Pi4PcieProofPhase::PostMailboxReset.reloads_vl805_firmware_after_perst());
    }

    #[test]
    fn root_init_never_trusts_status_bits_without_live_config_proof() {
        let ready = BCM2711_PCIE_STATUS_PORT
            | BCM2711_PCIE_STATUS_DL_ACTIVE
            | BCM2711_PCIE_STATUS_PHY_LINK_UP;

        assert!(!pcie_root_ready_fast_path_allowed(
            Pi4PcieProofPhase::Initial,
            ready
        ));
        assert!(!pcie_root_ready_fast_path_allowed(
            Pi4PcieProofPhase::PostMailboxReset,
            ready
        ));
    }

    #[test]
    fn vl805_ext_cfg_selector_targets_pi4_bus1_dev0_func0() {
        assert_eq!(VL805_PCI_DEV_ADDR, 1 << 20);
        assert_eq!(VL805_PCI_DEV_ADDR & 0x000f_f000, 0);
        assert_eq!(VL805_PCI_DEV_ADDR >> 20, 1);
    }

    #[test]
    fn post_mailbox_ext_cfg_retry_uses_live_hal_proof_gate() {
        assert!(vl805_post_mailbox_ext_cfg_retry_needed(
            RPI4_VL805_XHCI_MMIO,
            RPI4_VL805_XHCI_MMIO,
            false,
            true,
        ));
        assert!(!vl805_post_mailbox_ext_cfg_retry_needed(
            RPI4_VL805_XHCI_MMIO,
            RPI4_VL805_XHCI_MMIO,
            true,
            true,
        ));
        assert!(!vl805_post_mailbox_ext_cfg_retry_needed(
            0x0000_0000_fe98_0000,
            RPI4_VL805_XHCI_MMIO,
            false,
            true,
        ));
        assert!(!vl805_post_mailbox_ext_cfg_retry_needed(
            RPI4_VL805_XHCI_MMIO,
            RPI4_VL805_XHCI_MMIO,
            false,
            false,
        ));
    }

    #[test]
    fn post_mailbox_ext_cfg_data_read_waits_for_link_bits() {
        assert!(post_mailbox_ext_cfg_data_read_deferred(
            BCM2711_PCIE_STATUS_PORT
        ));
        assert!(post_mailbox_ext_cfg_data_read_deferred(0));
        assert!(post_mailbox_ext_cfg_data_read_deferred(
            BCM2711_PCIE_STATUS_DL_ACTIVE | BCM2711_PCIE_STATUS_PHY_LINK_UP
        ));
        assert!(!post_mailbox_ext_cfg_data_read_deferred(
            BCM2711_PCIE_STATUS_PORT
                | BCM2711_PCIE_STATUS_DL_ACTIVE
                | BCM2711_PCIE_STATUS_PHY_LINK_UP
        ));
    }

    #[test]
    fn bcm2711_pcie_register_pages_are_mapped_in_sel4_cursor_order() {
        let (root_cfg_page, _) = pcie_reg_page(PCI_CFG_VENDOR_DEVICE).expect("root cfg page");
        let (status_page, _) = pcie_reg_page(BCM2711_PCIE_MISC_PCIE_STATUS).expect("status page");
        let (ext_data_page, _) = pcie_reg_page(BCM2711_PCIE_EXT_CFG_DATA).expect("ext data page");
        let (ext_index_page, _) =
            pcie_reg_page(BCM2711_PCIE_EXT_CFG_INDEX).expect("ext index page");
        let (sw_init_page, _) = pcie_reg_page(BCM2711_PCIE_RGR1_SW_INIT_1).expect("sw init page");

        assert!(root_cfg_page < status_page);
        assert!(status_page < ext_data_page);
        assert!(ext_data_page < ext_index_page);
        assert_eq!(ext_index_page, sw_init_page);
    }

    #[test]
    fn vl805_bar_translation_uses_pi4_outbound_window() {
        assert_eq!(
            translate_vl805_pci_bar_to_cpu_mmio(0xc000_0004, 0),
            Some(RPI4_VL805_XHCI_MMIO)
        );
        assert_eq!(
            translate_vl805_pci_bar_to_cpu_mmio(0xc000_1004, 0),
            Some(RPI4_VL805_XHCI_MMIO + 0x1000)
        );
        assert_eq!(translate_vl805_pci_bar_to_cpu_mmio(0xb000_0004, 0), None);
    }

    #[test]
    fn vl805_xhci_port_access_is_bounded_to_root_port_window() {
        assert!(vl805_xhci_port_register_offset_valid(0x420, 0, 5));
        assert!(vl805_xhci_port_register_offset_valid(0x460, 4, 5));
        assert!(!vl805_xhci_port_register_offset_valid(0x3fc, 0, 5));
        assert!(!vl805_xhci_port_register_offset_valid(0x421, 0, 5));
        assert!(!vl805_xhci_port_register_offset_valid(0x424, 0, 5));
        assert!(!vl805_xhci_port_register_offset_valid(0x430, 0, 5));
        assert!(!vl805_xhci_port_register_offset_valid(0x420, 1, 5));
        assert!(!vl805_xhci_port_register_offset_valid(0x420, 5, 5));
        assert!(!vl805_xhci_port_register_offset_valid(0x420, 0, 0));
        assert!(!vl805_xhci_port_register_offset_valid(0x1_0000, 0, 5));
    }

    #[test]
    fn vl805_bar_assignment_is_limited_to_unassigned_64bit_memory_bar() {
        assert!(vl805_bar_assignment_needed(0x0000_0004, 0));
        assert_eq!(vl805_pi4_assigned_bar0_value(), 0xc000_0004);
        assert!(!vl805_bar_assignment_needed(0xc000_0004, 0));
        assert!(!vl805_bar_assignment_needed(0x0000_0000, 0));
        assert!(!vl805_bar_assignment_needed(0x0000_0005, 0));
        assert!(!vl805_bar_assignment_needed(0x0000_0004, 1));
    }

    #[test]
    fn vl805_post_command_reset_notify_follows_bar_command_or_devctl_changes() {
        let unchanged_devctl = Vl805PcieDeviceControlProof {
            control_before: Some(VL805_PCIE_DEVCTL_COMMAND_PROOF),
            control_after: Some(VL805_PCIE_DEVCTL_COMMAND_PROOF),
        };
        let changed_devctl = Vl805PcieDeviceControlProof {
            control_before: Some(0),
            control_after: Some(VL805_PCIE_DEVCTL_COMMAND_PROOF),
        };

        assert!(!vl805_post_command_reset_notify_needed(
            Pi4PcieProofPhase::Initial,
            0x0000_0004,
            0,
            0xc000_0004,
            0,
            0,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            changed_devctl,
        ));
        assert!(vl805_post_command_reset_notify_needed(
            Pi4PcieProofPhase::PostMailboxReset,
            0x0000_0004,
            0,
            0xc000_0004,
            0,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            unchanged_devctl,
        ));
        assert!(vl805_post_command_reset_notify_needed(
            Pi4PcieProofPhase::PostMailboxReset,
            0xc000_0004,
            0,
            0xc000_0004,
            0,
            0,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            unchanged_devctl,
        ));
        assert!(vl805_post_command_reset_notify_needed(
            Pi4PcieProofPhase::PostMailboxReset,
            0xc000_0004,
            0,
            0xc000_0004,
            0,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            changed_devctl,
        ));
        assert!(!vl805_post_command_reset_notify_needed(
            Pi4PcieProofPhase::PostMailboxReset,
            0xc000_0004,
            0,
            0xc000_0004,
            0,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            VL805_POLL_ONLY_COMMAND_REQUIRED,
            unchanged_devctl,
        ));
    }

    #[test]
    fn vl805_exact_ext_cfg_proof_requires_live_pi4_tuple() {
        let vendor_device = vl805_vendor_device_dword(VL805_PCI_VENDOR_ID, VL805_PCI_DEVICE_ID);
        let class_revision = VL805_EXPECTED_CLASS_CODE << 8;
        assert_eq!(vendor_device, 0x3483_1106);
        assert!(vl805_exact_config_tuple_ready(
            vendor_device,
            class_revision,
            0xc000_0004,
            0
        ));
        assert!(!vl805_exact_config_tuple_ready(
            0xffff_ffff,
            class_revision,
            0xc000_0004,
            0
        ));
        assert!(!vl805_exact_config_tuple_ready(
            vendor_device,
            0x0001_0000,
            0xc000_0004,
            0
        ));
        assert!(!vl805_exact_config_tuple_ready(
            vendor_device,
            class_revision,
            0xb000_0004,
            0
        ));
        assert!(vl805_ext_cfg_selector_echo(VL805_PCI_DEV_ADDR));
        assert!(!vl805_exact_config_tuple_ready(
            VL805_PCI_DEV_ADDR,
            class_revision,
            0xc000_0004,
            0
        ));
    }

    #[test]
    fn vl805_poll_only_command_requires_mem_master_and_intx_disabled() {
        let masked = vl805_poll_only_intx_mask_command(PCI_COMMAND_MEMORY_SPACE);
        assert_eq!(
            masked,
            PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_INTERRUPT_DISABLE
        );
        assert_eq!(
            vl805_poll_only_bus_master_command(masked),
            VL805_POLL_ONLY_COMMAND_REQUIRED
        );
        assert!(vl805_command_ownership_ready(
            VL805_POLL_ONLY_COMMAND_REQUIRED
        ));
        assert!(!vl805_command_ownership_ready(
            PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER
        ));
    }

    #[test]
    fn bcm2711_root_bridge_config_matches_linux_capture() {
        assert_eq!(RPI4_PCIE_BRIDGE_BUS_NUMBERS, 0x0001_0100);
        assert_eq!(RPI4_VL805_BRIDGE_MMIO_WINDOW_BYTES, 0x0010_0000);
        assert_eq!(rpi4_vl805_bridge_memory_base_limit(), 0xc000_c000);
        assert_eq!(
            pci_bridge_memory_base_limit(0xc000_0000, 0x0000_1000),
            0xc000_c000
        );
        assert_eq!(RPI4_PCIE_BRIDGE_IO_BASE_LIMIT_DISABLED, 0);
        assert_eq!(RPI4_PCIE_BRIDGE_PREFETCH_BASE_LIMIT_DISABLED, 0x0001_fff1);
        assert_eq!(
            RPI4_PCIE_BRIDGE_COMMAND_REQUIRED,
            PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER
        );
        assert_eq!(BCM2711_ROOT_BRIDGE_CLASS_CODE, 0x0006_0400);
        assert_eq!(BCM2711_PCIE_RC_CFG_VENDOR_VENDOR_SPECIFIC_REG1, 0x0188);
        assert_eq!(BCM2711_PCIE_RC_CFG_PRIV1_LINK_CAPABILITY, 0x04dc);
        assert_eq!(PCIE_RC_CFG_VENDOR_SPECIFIC_REG1_ENDIAN_MODE_BAR2_MASK, 0x0c);
        assert_eq!(PCIE_RC_CFG_PRIV1_LINK_CAPABILITY_ASPM_SUPPORT_MASK, 0x0c00);
    }

    #[test]
    fn vl805_msi_control_disable_clears_enable_bit_only() {
        assert_eq!(vl805_msi_control_disable_value(0x00a5), 0x00a4);
        assert!(vl805_msi_control_disabled(0x00a4));
        assert!(!vl805_msi_control_disabled(0x00a5));
    }

    #[test]
    fn vl805_msix_control_quiesce_matches_uboot_maskall_disable() {
        assert_eq!(
            vl805_msix_control_quiesce_value(PCI_MSIX_CONTROL_ENABLE | 0x003f),
            PCI_MSIX_CONTROL_MASKALL | 0x003f
        );
        assert!(vl805_msix_control_disabled(PCI_MSIX_CONTROL_MASKALL));
        assert!(!vl805_msix_control_disabled(PCI_MSIX_CONTROL_ENABLE));
        assert!(vl805_msix_control_quiesced(PCI_MSIX_CONTROL_MASKALL));
        assert!(!vl805_msix_control_quiesced(0));
        assert!(!vl805_msix_control_quiesced(PCI_MSIX_CONTROL_ENABLE));
    }

    #[test]
    fn vl805_pcie_devctl_command_proof_matches_linux_dma_read_attributes() {
        assert_eq!(VL805_PCIE_DEVCTL_LINUX_CAPTURE, 0x281f);
        assert_eq!(VL805_PCIE_DEVCTL_COMMAND_PROOF, 0x281f);
        assert_eq!(vl805_pcie_devctl_command_proof_value(0), 0x281f);
        assert_eq!(vl805_pcie_devctl_command_proof_value(0xffff), 0x281f);
        assert!(vl805_pcie_devctl_ready(0x281f));
        assert!(!vl805_pcie_devctl_ready(0x200f));
        assert!(!vl805_pcie_devctl_ready(0x081f));
        assert!(!vl805_pcie_devctl_ready(0x201f));
        assert!(!vl805_pcie_devctl_ready(0x181f));
    }

    #[test]
    fn vl805_absent_msi_or_msix_capability_is_poll_only_quiesced() {
        let proof = Pi4Vl805PcieProof {
            status: BCM2711_PCIE_STATUS_PORT
                | BCM2711_PCIE_STATUS_DL_ACTIVE
                | BCM2711_PCIE_STATUS_PHY_LINK_UP,
            config_virt: 0,
            vendor_id: VL805_PCI_VENDOR_ID,
            device_id: VL805_PCI_DEVICE_ID,
            class_code: VL805_EXPECTED_CLASS_CODE,
            command_before: 0,
            command_after: VL805_POLL_ONLY_COMMAND_REQUIRED,
            bar0: 0xc000_0004,
            bar1: 0,
            mmio: RPI4_VL805_XHCI_MMIO,
            msi_control_before: None,
            msi_control_after: None,
            msix_control_before: None,
            msix_control_after: None,
            pcie_devctl_before: Some(0),
            pcie_devctl_after: Some(VL805_PCIE_DEVCTL_COMMAND_PROOF),
        };
        assert!(proof.msi_disabled());
        assert!(proof.msix_quiesced());
        assert!(proof.interrupt_modes_quiesced());
        assert!(proof.pcie_device_control_ready());
    }

    #[test]
    fn vl805_posted_write_flush_skips_xhci_bar_drain_on_run_and_doorbells() {
        assert!(vl805_xhci_flush_is_doorbell(0x031f, 0x0100));
        assert!(vl805_xhci_flush_is_doorbell(0x031f, 0x0104));
        assert!(vl805_xhci_flush_is_run_write(0x02e5, 0x0020));
        assert!(vl805_xhci_flush_is_run_write(0x035c, 0x0020));
        assert!(vl805_xhci_flush_is_run_write(0x03eb, 0x0020));
        assert!(!vl805_xhci_flush_is_doorbell(0x02e5, 0x0020));
        assert!(!vl805_xhci_flush_is_run_write(0x02e5, 0x0100));
        assert!(!vl805_xhci_flush_is_doorbell(0x031f, 0x00fc));
        assert!(!vl805_xhci_flush_is_doorbell(0x031f, 0x0102));
        assert!(!vl805_xhci_flush_is_doorbell(0x031f, 0x0200));
        assert!(!vl805_xhci_flush_is_doorbell(0x031f, 0x0224));
        assert!(vl805_xhci_flush_skips_bar_drain(0x031f, 0x0100));
        assert!(vl805_xhci_flush_skips_bar_drain(0x031f, 0x0104));
        assert!(vl805_xhci_flush_skips_bar_drain(0x02e5, 0x0020));
        assert!(!vl805_xhci_flush_skips_bar_drain(0x031f, 0x00fc));
        assert!(!vl805_xhci_flush_skips_bar_drain(0x031f, 0x0224));
        assert!(!vl805_xhci_flush_skips_bar_drain(0x02e5, 0x0100));
        assert_eq!(vl805_xhci_flush_stage_role(0x0118, 0x0c08), "brcm-axiwra");
        assert_eq!(vl805_xhci_flush_stage_role(0x0119, 0x0c0c), "brcm-axirda");
        assert_eq!(
            vl805_xhci_flush_stage_role(0x031f, 0x010c),
            "endpoint-doorbell"
        );
        assert_eq!(
            vl805_xhci_flush_success_log_mask(0x031f, 0x010c),
            Some(VL805_FLUSH_LOG_ENDPOINT_DOORBELL)
        );
        assert_eq!(
            vl805_xhci_flush_success_log_mask(0x03fe, 0x0238),
            Some(VL805_FLUSH_LOG_RUNTIME_ERDP_LOW)
        );
        assert_eq!(
            vl805_xhci_flush_success_log_mask(0x03ff, 0x023c),
            Some(VL805_FLUSH_LOG_RUNTIME_ERDP_HIGH)
        );
        assert_eq!(vl805_xhci_flush_success_log_mask(0x031f, 0x0100), None);
        assert_eq!(vl805_xhci_flush_success_log_mask(0x0267, 0x0224), None);
        assert_eq!(vl805_xhci_flush_stage_role(0x0267, 0x0224), "generic-mmio");
        assert_eq!(vl805_xhci_flush_stage_role(0x0268, 0x0220), "generic-mmio");
        assert!(vl805_ext_cfg_flush_read_valid(
            VL805_PCI_DEV_ADDR,
            0x0018_0546
        ));
        assert!(!vl805_ext_cfg_flush_read_valid(0, 0x0018_0546));
        assert!(!vl805_ext_cfg_flush_read_valid(
            VL805_PCI_DEV_ADDR,
            VL805_PCI_DEV_ADDR
        ));
        assert!(!vl805_ext_cfg_flush_read_valid(
            VL805_PCI_DEV_ADDR,
            0xffff_ffff
        ));
    }

    #[test]
    fn vl805_posted_write_flush_requires_live_pcie_irq_and_command_proof() {
        let ready_command_status = 0x0018_0000 | u32::from(VL805_POLL_ONLY_COMMAND_REQUIRED);
        assert_eq!(
            vl805_xhci_flush_live_proof_failure(false, true, ready_command_status),
            Some("link-root-unproven")
        );
        assert_eq!(
            vl805_xhci_flush_live_proof_failure(true, false, ready_command_status),
            Some("irq-source-mask-unproven")
        );
        assert_eq!(
            vl805_xhci_flush_live_proof_failure(true, true, 0x0018_0000),
            Some("command-ownership")
        );
        assert_eq!(
            vl805_xhci_flush_live_proof_failure(true, true, ready_command_status),
            None
        );
    }

    #[test]
    fn pcie_owner_queue_record_is_fixed_layout_and_non_acceptance() {
        assert_eq!(core::mem::size_of::<Pi4PcieOwnerQueueRecord>(), 48);
        assert_eq!(core::mem::align_of::<Pi4PcieOwnerQueueRecord>(), 4);
        assert!(
            core::mem::size_of::<Pi4PcieOwnerQueueRecord>()
                <= super::super::driver_task::DRIVER_TASK_OWNER_STATE_BYTES
        );

        let before = pi4_pcie_owner_queue_record();
        let _ = pcie_owner_queue_submit(PCIE_OWNER_OP_PORT_WRITE, 0x031f, 0x0100, 0x1234_5678);
        let after = pi4_pcie_owner_queue_record();

        assert_eq!(after.version, PCIE_OWNER_QUEUE_RECORD_VERSION);
        assert_eq!(after.flags, PCIE_OWNER_QUEUE_FLAGS);
        assert!(
            after.flags
                & super::super::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE
                != 0
        );
        assert_eq!(after.depth, PCIE_OWNER_QUEUE_DEPTH as u16);
        assert_eq!(after.submitted, before.submitted.saturating_add(1));
        assert_eq!(after.last_op, PCIE_OWNER_OP_PORT_WRITE);
        assert_eq!(after.last_stage, 0x031f);
        assert_eq!(after.last_offset, 0x0100);
        assert_eq!(after.last_value, 0x1234_5678);
        assert!(!after.acceptance_eligible());
        assert_eq!(after.non_acceptance_reason(), "root-mmio-exec");
    }

    #[test]
    fn pcie_irq_source_mask_proof_rejects_sentinel_readbacks() {
        assert!(!pcie_irq_source_mask_readback_trusted(
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX
        ));
        assert!(!pcie_irq_source_mask_readback_trusted(0, u32::MAX, 0, 0));
        assert!(pcie_irq_source_mask_readback_trusted(0, 0, 0, 0));
        assert!(pcie_irq_source_mask_readback_trusted(
            0x0000_0001,
            0,
            0x0000_0002,
            0
        ));
    }

    #[test]
    fn failed_root_init_attempt_rearms_phase_latch_for_retry() {
        let phase = Pi4PcieProofPhase::PostMailboxReset;
        phase.root_init_latch().store(1, Ordering::Release);
        finish_pi4_pcie_root_init_attempt(phase, false);
        assert_eq!(phase.root_init_latch().load(Ordering::Acquire), 0);

        phase.root_init_latch().store(1, Ordering::Release);
        finish_pi4_pcie_root_init_attempt(phase, true);
        assert_eq!(phase.root_init_latch().load(Ordering::Acquire), 1);
        phase.root_init_latch().store(0, Ordering::Release);
    }

    #[test]
    fn bcm2711_root_window_values_match_pi4_dt_ranges() {
        assert_eq!(
            pcie_outbound_base_limit_value(
                RPI4_PCIE_CPU_MMIO_WINDOW_BASE as u64,
                RPI4_PCIE_MMIO_WINDOW_SIZE,
            ),
            0x3ff0_0000
        );
        assert_eq!(
            pcie_outbound_base_hi_value(RPI4_PCIE_CPU_MMIO_WINDOW_BASE as u64),
            0x06
        );
        assert_eq!(
            pcie_outbound_limit_hi_value(
                RPI4_PCIE_CPU_MMIO_WINDOW_BASE as u64,
                RPI4_PCIE_MMIO_WINDOW_SIZE,
            ),
            0x06
        );
        assert_eq!(RPI4_PCIE_BUS_MMIO_WINDOW_BASE, 0xc000_0000);
        assert_eq!(RPI4_PCIE_BUS_MMIO_WINDOW_BYTES, 0x4000_0000);
    }

    #[test]
    fn bcm2711_dma_window_values_match_pi4_dma_ranges() {
        let dma_size = pcie_next_power_of_two(RPI4_PCIE_DMA_WINDOW_BYTES);
        assert_eq!(dma_size, 0x1_0000_0000);
        assert_eq!(brcm_pcie_encode_ibar_size(dma_size), 17);
        let dma_offset = RPI4_PCIE_DMA_BUS_BASE - RPI4_PCIE_DMA_CPU_BASE;
        assert_eq!(dma_offset, 0x4_0000_0000);
        assert_eq!(
            replace_u32_field(
                dma_offset as u32,
                PCIE_MISC_RC_BAR2_CONFIG_LO_SIZE_MASK,
                brcm_pcie_encode_ibar_size(dma_size),
            ),
            17
        );
        assert_eq!((dma_offset >> 32) as u32, 4);
        assert_eq!(
            replace_u32_field(0, PCIE_MISC_MISC_CTRL_SCB0_SIZE_MASK, 17),
            0x8800_0000
        );
        assert_eq!(RPI4_PCIE_DMA_BUS_BASE, 0x0000_0004_0000_0000);
        assert_eq!(RPI4_PCIE_DMA_CPU_BASE, 0);
        assert_eq!(RPI4_PCIE_DMA_WINDOW_BYTES, 0x1_0000_0000);
    }
}
