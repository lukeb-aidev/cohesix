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
    DriverRuntimeCyw43CommandDescriptor, DriverRuntimeInitDescriptor,
    DriverRuntimeResourceRangeDescriptor, DriverRuntimeSdioCommandDescriptor,
    DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO, DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE,
    DRIVER_RUNTIME_CYW43_COMMAND_AUX, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
    DRIVER_RUNTIME_CYW43_OP_ETH_TX, DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
    DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK, DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL,
    DRIVER_RUNTIME_CYW43_OP_RELEASE, DRIVER_RUNTIME_CYW43_OP_RX_POLL,
    DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT, DRIVER_RUNTIME_ENGINE_INIT_AUX,
    DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_RGB888, DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
    DRIVER_RUNTIME_FRAMEBUFFER_VADDR, DRIVER_RUNTIME_INIT_AUX, DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX,
    DRIVER_RUNTIME_NET_INIT_AUX, DRIVER_RUNTIME_PCIE_OP_PORT_READ,
    DRIVER_RUNTIME_PCIE_OP_PORT_WRITE, DRIVER_RUNTIME_PCIE_OP_POSTED_WRITE_FLUSH,
    DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS, DRIVER_RUNTIME_RESOURCE_KIND_DMA,
    DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER, DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
    DRIVER_RUNTIME_RESOURCE_KIND_SHARED, DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
    DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL, DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS, DRIVER_RUNTIME_RESOURCE_TAG_HDMI_FRAMEBUFFER,
    DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST, DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
    DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL, DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI,
    DRIVER_RUNTIME_SDIO_FLAG_DATA, DRIVER_RUNTIME_SDIO_FLAG_RESP_LONG,
    DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE, DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR,
    DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT, DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY,
    DRIVER_RUNTIME_SDIO_FLAG_WRITE, DRIVER_RUNTIME_SDIO_OP_CMD52_READ,
    DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE, DRIVER_RUNTIME_SDIO_OP_CMD53_READ,
    DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE, DRIVER_RUNTIME_SDIO_OP_POLL_IRQ,
    DRIVER_RUNTIME_SDIO_RESP_LONG, DRIVER_RUNTIME_SDIO_RESP_NONE, DRIVER_RUNTIME_SDIO_RESP_OCR,
    DRIVER_RUNTIME_SDIO_RESP_SHORT, DRIVER_RUNTIME_SDIO_RESP_SHORT_BUSY, HOT_PATH_CYW43_WIFI,
    HOT_PATH_GENET_NIC, HOT_PATH_HDMI_TEXT, HOT_PATH_PCIE_ROOT, HOT_PATH_SDIO_HOST,
    HOT_PATH_SERIAL_CONSOLE, HOT_PATH_USB_KEYBOARD,
};

/// Child CSpace slot containing the root-to-driver command endpoint.
pub const DRIVER_TASK_CHILD_COMMAND_SLOT: sel4_sys::seL4_CPtr = 2;
/// Driver-local fixed virtual address for the command/completion ring.
pub const DRIVER_TASK_RING_VADDR: usize = 0x7000_0000;
/// First fixed driver-local virtual address reserved for explicit MMIO pages.
pub const DRIVER_TASK_DEVICE_MMIO_VADDR: usize = 0x7020_0000;
/// First fixed driver-local virtual address reserved for runtime-owned DMA pages.
pub const DRIVER_TASK_DMA_BUFFER_VADDR: usize = 0x7080_0000;
/// First fixed driver-local virtual address reserved for shared control pages.
pub const DRIVER_TASK_SHARED_BUFFER_VADDR: usize = 0x70c0_0000;
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
const ENGINE_STATE_HW_READY: u32 = 1 << 5;

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

const USB_REQUIRED_MMIO_PAGES: u16 = 16;
const USB_REQUIRED_DMA_PAGES: u16 = 128;
const USB_REQUIRED_SHARED_PAGES: u16 = 32;
const HDMI_REQUIRED_MMIO_PAGES: u16 = 1;
const HDMI_REQUIRED_DMA_PAGES: u16 = 16;
const HDMI_REQUIRED_SHARED_PAGES: u16 = 16;
const GENET_REQUIRED_MMIO_PAGES: u16 = 6;
const GENET_REQUIRED_DMA_PAGES: u16 = 512;
const GENET_REQUIRED_SHARED_PAGES: u16 = 32;
const CYW43_REQUIRED_MMIO_PAGES: u16 = 1;
const CYW43_REQUIRED_DMA_PAGES: u16 = 128;
const CYW43_REQUIRED_SHARED_PAGES: u16 = 64;
const SDIO_REQUIRED_MMIO_PAGES: u16 = 1;
const SDIO_REQUIRED_DMA_PAGES: u16 = 64;
const SDIO_REQUIRED_SHARED_PAGES: u16 = 32;
const PCIE_REQUIRED_MMIO_PAGES: u16 = 10;
const PCIE_REQUIRED_SHARED_PAGES: u16 = 16;
const PCIE_MMIO_ACCESS_BYTES: usize = core::mem::size_of::<u32>();

const GENET_SYS_OFF: usize = 0x0000;
const GENET_EXT_OFF: usize = 0x0080;
const GENET_RBUF_OFF: usize = 0x0300;
const GENET_UMAC_OFF: usize = 0x0800;
const GENET_RX_OFF: usize = 0x2000;
const GENET_TX_OFF: usize = 0x4000;
const GENET_SYS_PORT_CTRL: usize = GENET_SYS_OFF + 0x04;
const GENET_SYS_RBUF_FLUSH_CTRL: usize = GENET_SYS_OFF + 0x08;
const GENET_EXT_RGMII_OOB_CTRL: usize = GENET_EXT_OFF + 0x0c;
const GENET_RBUF_CTRL: usize = GENET_RBUF_OFF;
const GENET_RBUF_TBUF_SIZE_CTRL: usize = GENET_RBUF_OFF + 0xb4;
const GENET_UMAC_CMD: usize = GENET_UMAC_OFF + 0x08;
const GENET_UMAC_MAC0: usize = GENET_UMAC_OFF + 0x0c;
const GENET_UMAC_MAC1: usize = GENET_UMAC_OFF + 0x10;
const GENET_UMAC_MAX_FRAME_LEN: usize = GENET_UMAC_OFF + 0x14;
const GENET_UMAC_TX_FLUSH: usize = GENET_UMAC_OFF + 0x334;
const GENET_UMAC_MIB_CTRL: usize = GENET_UMAC_OFF + 0x580;
const GENET_MDIO_CMD: usize = GENET_UMAC_OFF + 0x614;
const GENET_CMD_TX_EN: u32 = 1 << 0;
const GENET_CMD_RX_EN: u32 = 1 << 1;
const GENET_CMD_SPEED_SHIFT: u32 = 2;
const GENET_CMD_SPEED_MASK: u32 = 0x3;
const GENET_CMD_SW_RESET: u32 = 1 << 13;
const GENET_CMD_LCL_LOOP_EN: u32 = 1 << 15;
const GENET_PORT_MODE_EXT_GPHY: u32 = 3;
const GENET_OOB_DISABLE: u32 = 1 << 5;
const GENET_RGMII_LINK: u32 = 1 << 4;
const GENET_RGMII_MODE_EN: u32 = 1 << 6;
const GENET_ID_MODE_DIS: u32 = 1 << 16;
const GENET_RBUF_ALIGN_2B: u32 = 1 << 1;
const GENET_MIB_RESET_RX: u32 = 1 << 0;
const GENET_MIB_RESET_RUNT: u32 = 1 << 1;
const GENET_MIB_RESET_TX: u32 = 1 << 2;
const GENET_MDIO_START_BUSY: u32 = 1 << 29;
const GENET_MDIO_READ_FAIL: u32 = 1 << 28;
const GENET_MDIO_RD: u32 = 2 << 26;
const GENET_MDIO_WR: u32 = 1 << 26;
const GENET_MDIO_PMD_SHIFT: u32 = 21;
const GENET_MDIO_REG_SHIFT: u32 = 16;
const GENET_MDIO_FIELD_MASK: u32 = 0x1f;
const GENET_MDIO_POLL_TRIES: usize = 10_000;
const GENET_MII_BMCR: u8 = 0;
const GENET_MII_BMSR: u8 = 1;
const GENET_MII_ADVERTISE: u8 = 4;
const GENET_MII_LPA: u8 = 5;
const GENET_MII_CTRL1000: u8 = 9;
const GENET_MII_STAT1000: u8 = 10;
const GENET_MII_PHYSID1: u8 = 2;
const GENET_MII_PHYSID2: u8 = 3;
const GENET_MII_BMSR_LSTATUS: u16 = 1 << 2;
const GENET_MII_BMSR_ANEGCOMPLETE: u16 = 1 << 5;
const GENET_MII_BMCR_SPEED100: u16 = 1 << 13;
const GENET_MII_BMCR_SPEED1000: u16 = 1 << 6;
const GENET_MII_BMCR_ANENABLE: u16 = 1 << 12;
const GENET_MII_BMCR_ANRESTART: u16 = 1 << 9;
const GENET_LPA_10HALF: u16 = 0x0020;
const GENET_LPA_10FULL: u16 = 0x0040;
const GENET_LPA_100HALF: u16 = 0x0080;
const GENET_LPA_100FULL: u16 = 0x0100;
const GENET_ADVERTISE_1000HALF: u16 = 0x0100;
const GENET_ADVERTISE_1000FULL: u16 = 0x0200;
const GENET_LPA_1000HALF: u16 = 0x0400;
const GENET_LPA_1000FULL: u16 = 0x0800;
const GENET_PHY_LINK_POLL_TRIES: usize = 256;
const GENET_PHY_LINK_POLL_DELAY_SPINS: usize = 4_096;
const GENET_DMA_EN: u32 = 1 << 0;
const GENET_DMA_RING_BUF_EN_SHIFT: u32 = 1;
const GENET_DMA_BUFLENGTH_SHIFT: u32 = 16;
const GENET_DMA_BUFLENGTH_MASK: u32 = 0x0fff;
const GENET_DMA_RING_SIZE_SHIFT: u32 = 16;
const GENET_DMA_OWN: u32 = 0x8000;
const GENET_DMA_EOP: u32 = 0x4000;
const GENET_DMA_SOP: u32 = 0x2000;
const GENET_DMA_TX_APPEND_CRC: u32 = 0x0040;
const GENET_DMA_TX_QTAG_SHIFT: u32 = 7;
const GENET_DMA_DEFAULT_QTAG: u32 = 0x3f;
const GENET_DMA_MAX_BURST_LENGTH: u32 = 0x8;
const GENET_DMA_DESC_SIZE: usize = 12;
const GENET_DMA_RING_SIZE: usize = 0x40;
const GENET_DEFAULT_Q: u32 = 16;
const GENET_HW_TOTAL_DESCS: usize = 256;
const GENET_RDMA_REG_OFF: usize = GENET_RX_OFF + GENET_HW_TOTAL_DESCS * GENET_DMA_DESC_SIZE;
const GENET_TDMA_REG_OFF: usize = GENET_TX_OFF + GENET_HW_TOTAL_DESCS * GENET_DMA_DESC_SIZE;
const GENET_DMA_RINGS_SIZE: usize = GENET_DMA_RING_SIZE * ((GENET_DEFAULT_Q as usize) + 1);
const GENET_RDMA_RING_REG_BASE: usize =
    GENET_RDMA_REG_OFF + (GENET_DEFAULT_Q as usize) * GENET_DMA_RING_SIZE;
const GENET_TDMA_RING_REG_BASE: usize =
    GENET_TDMA_REG_OFF + (GENET_DEFAULT_Q as usize) * GENET_DMA_RING_SIZE;
const GENET_RDMA_REG_BASE: usize = GENET_RDMA_REG_OFF + GENET_DMA_RINGS_SIZE;
const GENET_TDMA_REG_BASE: usize = GENET_TDMA_REG_OFF + GENET_DMA_RINGS_SIZE;
const GENET_DMA_RING_CFG: usize = 0x00;
const GENET_DMA_CTRL: usize = 0x04;
const GENET_DMA_SCB_BURST_SIZE: usize = 0x0c;
const GENET_DMA_RING_BUF_SIZE: usize = 0x10;
const GENET_DMA_START_ADDR: usize = 0x14;
const GENET_DMA_END_ADDR: usize = 0x1c;
const GENET_DMA_MBUF_DONE_THRESH: usize = 0x24;
const GENET_TDMA_FLOW_PERIOD: usize = GENET_TDMA_RING_REG_BASE + 0x28;
const GENET_TDMA_READ_PTR: usize = GENET_TDMA_RING_REG_BASE;
const GENET_TDMA_CONS_INDEX: usize = GENET_TDMA_RING_REG_BASE + 0x08;
const GENET_TDMA_PROD_INDEX: usize = GENET_TDMA_RING_REG_BASE + 0x0c;
const GENET_TDMA_WRITE_PTR: usize = GENET_TDMA_RING_REG_BASE + 0x2c;
const GENET_RDMA_WRITE_PTR: usize = GENET_RDMA_RING_REG_BASE;
const GENET_RDMA_PROD_INDEX: usize = GENET_RDMA_RING_REG_BASE + 0x08;
const GENET_RDMA_CONS_INDEX: usize = GENET_RDMA_RING_REG_BASE + 0x0c;
const GENET_RDMA_XON_XOFF_THRESH: usize = GENET_RDMA_RING_REG_BASE + 0x28;
const GENET_RDMA_READ_PTR: usize = GENET_RDMA_RING_REG_BASE + 0x2c;
const GENET_DMA_FC_THRESH_LO: u32 = 5;
const GENET_RX_BUF_LENGTH: usize = 2048;
const GENET_RX_BUF_OFFSET: usize = 2;
const GENET_MAX_FRAME_LEN: usize = 1536;
const GENET_RX_DRAIN_BUDGET: usize = 8;
const GENET_TX_COMPLETION_RECLAIM_BUDGET: usize = 32;
const GENET_DRIVER_TASK_MAC: [u8; 6] = [0x02, 0x43, 0x4f, 0x48, 0x58, 0x31];

const PCIE_MISC_MISC_CTRL: usize = 0x4008;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LO: usize = 0x400c;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_HI: usize = 0x4010;
const PCIE_MISC_RC_BAR1_CONFIG_LO: usize = 0x402c;
const PCIE_MISC_RC_BAR2_CONFIG_LO: usize = 0x4034;
const PCIE_MISC_RC_BAR2_CONFIG_HI: usize = 0x4038;
const PCIE_MISC_RC_BAR3_CONFIG_LO: usize = 0x403c;
const PCIE_MISC_PCIE_STATUS: usize = 0x4068;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT: usize = 0x4070;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_HI: usize = 0x4080;
const PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LIMIT_HI: usize = 0x4084;
const PCIE_MISC_HARD_PCIE_HARD_DEBUG: usize = 0x4204;
const PCIE_INTR2_CPU_CLR: usize = 0x4308;
const PCIE_INTR2_CPU_MASK_SET: usize = 0x4310;
const PCIE_MSI_INTR2_CLR: usize = 0x4508;
const PCIE_MSI_INTR2_MASK_SET: usize = 0x4510;
const PCIE_EXT_CFG_DATA: usize = 0x8000;
const PCIE_EXT_CFG_INDEX: usize = 0x9000;
const PCIE_RGR1_SW_INIT_1: usize = 0x9210;
const PCIE_MISC_MISC_CTRL_SCB_ACCESS_EN_MASK: u32 = 0x1000;
const PCIE_MISC_MISC_CTRL_CFG_READ_UR_MODE_MASK: u32 = 0x2000;
const PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_MASK: u32 = 0x300000;
const PCIE_MISC_MISC_CTRL_SCB0_SIZE_MASK: u32 = 0xf8000000;
const PCIE_MISC_RC_BAR1_CONFIG_LO_SIZE_MASK: u32 = 0x1f;
const PCIE_MISC_RC_BAR2_CONFIG_LO_SIZE_MASK: u32 = 0x1f;
const PCIE_MISC_RC_BAR3_CONFIG_LO_SIZE_MASK: u32 = 0x1f;
const PCIE_HARD_DEBUG_SERDES_IDDQ_MASK: u32 = 0x0800_0000;
const PCIE_RGR1_SW_INIT_1_INIT_MASK: u32 = 0x2;
const PCIE_RGR1_SW_INIT_1_PERST_MASK: u32 = 0x1;
const PCIE_STATUS_PORT: u32 = 0x80;
const PCIE_STATUS_DL_ACTIVE: u32 = 0x20;
const PCIE_STATUS_PHY_LINK_UP: u32 = 0x10;
const PCIE_VL805_PCI_DEV_ADDR: u32 = 0x0010_0000;
const PCIE_VL805_PCI_VENDOR_DEVICE: u32 = 0x3483_1106;
const PCIE_VL805_EXPECTED_CLASS_REV: u32 = 0x000c_0330 << 8;
const PCIE_CFG_COMMAND: usize = 0x04;
const PCIE_CFG_CLASS_REV: usize = 0x08;
const PCIE_CFG_BAR0: usize = 0x10;
const PCIE_CFG_BAR1: usize = 0x14;
const PCIE_VL805_ASSIGNED_BAR0: u32 = 0xc000_0004;
const PCIE_COMMAND_MEMORY_SPACE: u32 = 1 << 1;
const PCIE_COMMAND_BUS_MASTER: u32 = 1 << 2;
const PCIE_COMMAND_INTX_DISABLE: u32 = 1 << 10;
const PCIE_POLL_SPINS: usize = 50_000;

const XHCI_CAPLENGTH: usize = 0x00;
const XHCI_HCSPARAMS1: usize = 0x04;
const XHCI_HCSPARAMS2: usize = 0x08;
const XHCI_HCCPARAMS1: usize = 0x10;
const XHCI_DBOFF: usize = 0x14;
const XHCI_RTSOFF: usize = 0x18;
const XHCI_USBCMD: usize = 0x00;
const XHCI_USBSTS: usize = 0x04;
const XHCI_DNCTRL: usize = 0x14;
const XHCI_CRCR: usize = 0x18;
const XHCI_DCBAAP: usize = 0x30;
const XHCI_CONFIG: usize = 0x38;
const XHCI_USBCMD_RUN: u32 = 1;
const XHCI_USBCMD_HCRST: u32 = 1 << 1;
const XHCI_USBSTS_HCH: u32 = 1;
const XHCI_USBSTS_CNR: u32 = 1 << 11;
const XHCI_ERSTSZ: usize = 0x08;
const XHCI_ERSTBA: usize = 0x10;
const XHCI_ERDP: usize = 0x18;
const XHCI_IMAN: usize = 0x00;
const XHCI_DMA_DCBBA_OFFSET: usize = 0x0000;
const XHCI_DMA_COMMAND_RING_OFFSET: usize = 0x1000;
const XHCI_DMA_EVENT_RING_OFFSET: usize = 0x2000;
const XHCI_DMA_ERST_OFFSET: usize = 0x3000;
const XHCI_DMA_SCRATCHPAD_ARRAY_OFFSET: usize = 0x4000;
const XHCI_DMA_SCRATCHPAD_OFFSET: usize = 0x5000;
const XHCI_DMA_INPUT_CONTEXT_OFFSET: usize = 0x10000;
const XHCI_DMA_DEVICE_CONTEXT_OFFSET: usize = 0x12000;
const XHCI_DMA_EP0_RING_OFFSET: usize = 0x14000;
const XHCI_DMA_KBD_RING_OFFSET: usize = 0x15000;
const XHCI_DMA_CONTROL_BUFFER_OFFSET: usize = 0x16000;
const XHCI_DMA_CONFIG_BUFFER_OFFSET: usize = 0x17000;
const XHCI_DMA_REPORT_BUFFER_OFFSET: usize = 0x18000;
const XHCI_COMMAND_RING_TRBS: usize = 256;
const XHCI_EVENT_RING_TRBS: usize = 256;
const XHCI_TRB_BYTES: usize = 16;
const XHCI_PORTSC_BASE: usize = 0x400;
const XHCI_PORTSC_STRIDE: usize = 0x10;
const XHCI_PORTSC_CCS: u32 = 1 << 0;
const XHCI_PORTSC_PED: u32 = 1 << 1;
const XHCI_PORTSC_PR: u32 = 1 << 4;
const XHCI_PORTSC_PP: u32 = 1 << 9;
const XHCI_PORTSC_SPEED_SHIFT: u32 = 10;
const XHCI_PORTSC_SPEED_MASK: u32 = 0xf;
const XHCI_PORTSC_CSC: u32 = 1 << 17;
const XHCI_PORTSC_PEC: u32 = 1 << 18;
const XHCI_PORTSC_PRC: u32 = 1 << 21;
const XHCI_TRB_TYPE_NORMAL: u32 = 1;
const XHCI_TRB_TYPE_SETUP_STAGE: u32 = 2;
const XHCI_TRB_TYPE_DATA_STAGE: u32 = 3;
const XHCI_TRB_TYPE_STATUS_STAGE: u32 = 4;
const XHCI_TRB_TYPE_LINK: u32 = 6;
const XHCI_TRB_TYPE_ENABLE_SLOT: u32 = 9;
const XHCI_TRB_TYPE_ADDRESS_DEVICE: u32 = 11;
const XHCI_TRB_TYPE_CONFIGURE_ENDPOINT: u32 = 12;
const XHCI_TRB_TYPE_TRANSFER_EVENT: u32 = 32;
const XHCI_TRB_TYPE_COMMAND_COMPLETION: u32 = 33;
const XHCI_TRB_CYCLE: u32 = 1 << 0;
const XHCI_TRB_ENT: u32 = 1 << 1;
const XHCI_TRB_IOC: u32 = 1 << 5;
const XHCI_TRB_IDT: u32 = 1 << 6;
const XHCI_TRB_DIR_IN: u32 = 1 << 16;
const XHCI_TRB_TRANSFER_TYPE_OUT: u32 = 2 << 16;
const XHCI_TRB_TRANSFER_TYPE_IN: u32 = 3 << 16;
const XHCI_COMPLETION_SUCCESS: u32 = 1;
const XHCI_COMPLETION_SHORT_PACKET: u32 = 13;
const XHCI_ENDPOINT_TYPE_CONTROL: u32 = 4;
const XHCI_ENDPOINT_TYPE_INTERRUPT_IN: u32 = 7;
const XHCI_CONTEXT_ENTRIES_SHIFT: u32 = 27;
const XHCI_SLOT_SPEED_SHIFT: u32 = 20;
const XHCI_SLOT_ROOT_HUB_PORT_SHIFT: u32 = 16;
const XHCI_EP_TYPE_SHIFT: u32 = 3;
const XHCI_EP_MAX_PACKET_SHIFT: u32 = 16;
const XHCI_DEFAULT_CONTROL_PACKET: u32 = 64;
const XHCI_BOOT_REPORT_BYTES: usize = 8;
const XHCI_SETUP_GET_DESCRIPTOR: u8 = 6;
const XHCI_SETUP_SET_CONFIGURATION: u8 = 9;
const XHCI_SETUP_SET_IDLE: u8 = 10;
const XHCI_SETUP_SET_PROTOCOL: u8 = 11;
const USB_DESCRIPTOR_DEVICE: u8 = 1;
const USB_DESCRIPTOR_CONFIGURATION: u8 = 2;
const USB_DESCRIPTOR_INTERFACE: u8 = 4;
const USB_DESCRIPTOR_ENDPOINT: u8 = 5;
const USB_CLASS_HID: u8 = 3;
const USB_SUBCLASS_BOOT: u8 = 1;
const USB_PROTOCOL_KEYBOARD: u8 = 1;
const USB_ENDPOINT_ATTR_INTERRUPT: u8 = 3;
const USB_ENDPOINT_DIR_IN: u8 = 0x80;
const USB_XHCI_SPINS: usize = 100_000;

const SDHCI_HOST_CONTROL: usize = 0x28;
const SDHCI_POWER_CONTROL: usize = 0x29;
const SDHCI_CLOCK_CONTROL: usize = 0x2c;
const SDHCI_TIMEOUT_CONTROL: usize = 0x2e;
const SDHCI_SOFTWARE_RESET: usize = 0x2f;
const SDHCI_INT_ENABLE: usize = 0x34;
const SDHCI_SIGNAL_ENABLE: usize = 0x38;
const SDHCI_POWER_ON: u8 = 0x01;
const SDHCI_POWER_330: u8 = 0x0e;
const SDHCI_CLOCK_INT_EN: u16 = 1 << 0;
const SDHCI_CLOCK_INT_STABLE: u16 = 1 << 1;
const SDHCI_CLOCK_CARD_EN: u16 = 1 << 2;
const SDHCI_RESET_ALL: u8 = 0x01;
const SDHCI_RESET_CMD: u8 = 0x02;
const SDHCI_RESET_DATA: u8 = 0x04;
const SDHCI_INIT_SPINS: usize = 100_000;

const CYW43_SDPCM_HEADER_BYTES: usize = 12;
const CYW43_BDC_HEADER_BYTES: usize = 4;
const CYW43_BUS_HEADER_BYTES: usize = CYW43_SDPCM_HEADER_BYTES + CYW43_BDC_HEADER_BYTES;
const CYW43_RAM_BASE_4345: u32 = 0x0019_8000;
const CYW43_RAM_SIZE_4345_PI4: u32 = 0x000c_8000;
const CYW43_FIRMWARE_RESET_VECTOR_ADDR: u32 = 0x0000_0000;
const CYW43_ARMCR4_CORE_BASE: u32 = 0x1810_2000;
const CYW43_SDIO_CORE_BASE: u32 = 0x1800_4000;
const BACKPLANE_ADDRESS_MASK: u32 = 0x7fff;
const BACKPLANE_WINDOW_MASK: u32 = 0xffff_8000;
const BACKPLANE_32BIT_FLAG: u32 = 0x8000;
const AI_IOCTRL_OFFSET: u32 = 0x408;
const AI_RESETCTRL_OFFSET: u32 = 0x800;
const AI_IOCTRL_BIT_CLOCK_EN: u8 = 0x01;
const AI_IOCTRL_BIT_FGC: u8 = 0x02;
const AI_CORE_PRERESET_IOCTRL: u8 = AI_IOCTRL_BIT_FGC | AI_IOCTRL_BIT_CLOCK_EN;
const AI_CORE_POSTRESET_IOCTRL: u8 = AI_IOCTRL_BIT_CLOCK_EN;
const AI_RESETCTRL_BIT_RESET: u8 = 0x01;
const ARMCR4_BCMA_IOCTL_CPUHALT: u8 = 0x20;
const SDIO_CMD5: u16 = 5;
const SDIO_CMD3: u16 = 3;
const SDIO_CMD7: u16 = 7;
const SDIO_CMD52: u16 = 52;
const SDIO_CMD53: u16 = 53;
const SDIO_R4_READY: u32 = 1 << 31;
const SDIO_OCR_3V2_3V4: u32 = 0x00ff_8000;
const SDIO_CCCR_IOEX: u32 = 0x02;
const SDIO_CCCR_IORX: u32 = 0x03;
const SDIO_CCCR_IF: u32 = 0x07;
const SDIO_CCCR_FBR_BASE: u32 = 0x100;
const SDIO_FBR_BLKSIZE: u32 = 0x10;
const SDIO_BUS_WIDTH_4BIT: u8 = 0x02;
const SDIO_FUNC_ENABLE_1: u8 = 0x02;
const SDIO_FUNC_ENABLE_2: u8 = 0x04;
const SDIO_FUNC_READY_1: u8 = 0x02;
const SDIO_FUNC_READY_2: u8 = 0x04;
const SDIO_FUNCTION1_BLOCK_SIZE: u16 = 64;
const SDIO_FUNCTION2_BLOCK_SIZE: u16 = 512;
const SBSDIO_WATERMARK: u32 = 0x10008;
const SBSDIO_DEVICE_CTL: u32 = 0x10009;
const SBSDIO_DEVCTL_F2WM_ENAB: u8 = 0x10;
const SBSDIO_FUNC1_SBADDRLOW: u32 = 0x1000a;
const SBSDIO_FUNC1_SBADDRMID: u32 = 0x1000b;
const SBSDIO_FUNC1_SBADDRHIGH: u32 = 0x1000c;
const SBSDIO_FUNC1_CHIPCLKCSR: u32 = 0x1000e;
const SBSDIO_FUNC1_RFRAMEBCLO: u32 = 0x1001b;
const SBSDIO_FUNC1_RFRAMEBCHI: u32 = 0x1001c;
const SBSDIO_FUNC1_MESBUSYCTRL: u32 = 0x1001d;
const SBSDIO_FUNC1_WAKEUPCTRL: u32 = 0x1001e;
const SBSDIO_FUNC1_SLEEPCSR: u32 = 0x1001f;
const SBSDIO_ALP_AVAIL_REQ: u8 = 0x08;
const SBSDIO_HT_AVAIL_REQ: u8 = 0x10;
const SBSDIO_ALP_AVAIL: u8 = 0x40;
const SBSDIO_HT_AVAIL: u8 = 0x80;
const SBSDIO_WAKE_TILL_HT_AVAIL: u8 = 0x02;
const SBSDIO_FUNC1_SLEEPCSR_KSO_EN: u8 = 0x01;
const CY_43455_F2_WATERMARK: u8 = 0x60;
const CY_43455_MESBUSYCTRL: u8 = 0xd0;
const SDIO_CORECONTROL: u32 = 0x00;
const SDPCMD_REG_HOSTINTMASK: u32 = 0x24;
const SDPCMD_REG_FUNCTIONINTMASK: u32 = 0x34;
const CC_F2RDY: u32 = 1 << 2;
const I_HMB_SW_MASK: u32 = 0x0000_00f0;
const I_CHIPACTIVE: u32 = 1 << 29;
const HOSTINTMASK: u32 = I_HMB_SW_MASK | I_CHIPACTIVE;
const FUNCTIONINTMASK: u32 = SDIO_FUNC_ENABLE_2 as u32;

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

struct RuntimeStateSlot<T> {
    state: UnsafeCell<T>,
}

// SAFETY: Linked driver runtimes are single-TCB service loops. Host tests
// serialize access through `test_guard`, and target code never shares these
// cells with root.
unsafe impl<T> Sync for RuntimeStateSlot<T> {}

impl<T> RuntimeStateSlot<T> {
    const fn new(state: T) -> Self {
        Self {
            state: UnsafeCell::new(state),
        }
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        // SAFETY: See the `Sync` invariant; there is one runtime service turn
        // at a time and no references escape this closure.
        unsafe { f(&mut *self.state.get()) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GenetRuntimeState {
    initialized: bool,
    tx_prod_index: u16,
    tx_cons_index: u16,
    rx_cons_index: u16,
    phy_addr: u8,
    link_ready: bool,
    link_speed: u32,
    tx_packets: u32,
    rx_packets: u32,
    tx_drops: u32,
}

impl GenetRuntimeState {
    const fn new() -> Self {
        Self {
            initialized: false,
            tx_prod_index: 0,
            tx_cons_index: 0,
            rx_cons_index: 0,
            phy_addr: 0xff,
            link_ready: false,
            link_speed: 0,
            tx_packets: 0,
            rx_packets: 0,
            tx_drops: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PcieRuntimeState {
    initialized: bool,
    link_ready: bool,
    last_status: u32,
    cfg_vendor_device: u32,
    cfg_class_revision: u32,
    op_count: u32,
}

impl PcieRuntimeState {
    const fn new() -> Self {
        Self {
            initialized: false,
            link_ready: false,
            last_status: 0,
            cfg_vendor_device: 0,
            cfg_class_revision: 0,
            op_count: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UsbRuntimeState {
    initialized: bool,
    context_bytes: usize,
    scratchpad_count: usize,
    cap_length: u8,
    max_slots: u8,
    max_ports: u8,
    db_offset: u32,
    rt_offset: u32,
    cmd_enqueue: u16,
    cmd_cycle: bool,
    event_dequeue: u16,
    event_cycle: bool,
    ep0_enqueue: u16,
    ep0_cycle: bool,
    kbd_enqueue: u16,
    kbd_cycle: bool,
    keyboard_slot: u8,
    keyboard_port: u8,
    keyboard_endpoint_id: u8,
    keyboard_endpoint_address: u8,
    keyboard_interface: u8,
    keyboard_ep_interval: u8,
    keyboard_ep_max_packet: u16,
    keyboard_report_queued: bool,
    last_keys: [u8; 6],
    reports: u32,
}

impl UsbRuntimeState {
    const fn new() -> Self {
        Self {
            initialized: false,
            context_bytes: 32,
            scratchpad_count: 0,
            cap_length: 0,
            max_slots: 0,
            max_ports: 0,
            db_offset: 0,
            rt_offset: 0,
            cmd_enqueue: 0,
            cmd_cycle: true,
            event_dequeue: 0,
            event_cycle: true,
            ep0_enqueue: 0,
            ep0_cycle: true,
            kbd_enqueue: 0,
            kbd_cycle: true,
            keyboard_slot: 0,
            keyboard_port: 0,
            keyboard_endpoint_id: 0,
            keyboard_endpoint_address: 0,
            keyboard_interface: 0,
            keyboard_ep_interval: 0,
            keyboard_ep_max_packet: 0,
            keyboard_report_queued: false,
            last_keys: [0; 6],
            reports: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SdioRuntimeState {
    initialized: bool,
    commands: u32,
    last_response: u32,
}

impl SdioRuntimeState {
    const fn new() -> Self {
        Self {
            initialized: false,
            commands: 0,
            last_response: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43RuntimeState {
    initialized: bool,
    transport_ready: bool,
    firmware_uploaded: bool,
    nvram_uploaded: bool,
    firmware_released: bool,
    sdpcm_seq: u8,
    sdpcm_seq_max: u8,
    tx_frames: u32,
    rx_frames: u32,
    firmware_bytes: u32,
    nvram_bytes: u32,
    backplane_window: u32,
    backplane_window_valid: bool,
    bus_link_ready: bool,
}

impl Cyw43RuntimeState {
    const fn new() -> Self {
        Self {
            initialized: false,
            transport_ready: false,
            firmware_uploaded: false,
            nvram_uploaded: false,
            firmware_released: false,
            sdpcm_seq: 0,
            sdpcm_seq_max: 1,
            tx_frames: 0,
            rx_frames: 0,
            firmware_bytes: 0,
            nvram_bytes: 0,
            backplane_window: 0,
            backplane_window_valid: false,
            bus_link_ready: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
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
static SDIO_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static SDIO_CMD_COUNT: AtomicU32 = AtomicU32::new(0);
static PCIE_RUNTIME_FLAGS: AtomicU32 = AtomicU32::new(0);
static PCIE_OP_COUNT: AtomicU32 = AtomicU32::new(0);
static GENET_RUNTIME_STATE: RuntimeStateSlot<GenetRuntimeState> =
    RuntimeStateSlot::new(GenetRuntimeState::new());
static PCIE_RUNTIME_STATE: RuntimeStateSlot<PcieRuntimeState> =
    RuntimeStateSlot::new(PcieRuntimeState::new());
static USB_RUNTIME_STATE: RuntimeStateSlot<UsbRuntimeState> =
    RuntimeStateSlot::new(UsbRuntimeState::new());
static SDIO_RUNTIME_STATE: RuntimeStateSlot<SdioRuntimeState> =
    RuntimeStateSlot::new(SdioRuntimeState::new());
static CYW43_RUNTIME_STATE: RuntimeStateSlot<Cyw43RuntimeState> =
    RuntimeStateSlot::new(Cyw43RuntimeState::new());

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
        HOT_PATH_GENET_NIC => service_genet_runtime(command),
        HOT_PATH_CYW43_WIFI => service_cyw43_runtime(command),
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
    if !runtime_engine_init(hot_path, descriptor) {
        return false;
    }
    let mut bits = ENGINE_STATE_INITIALIZED | ENGINE_STATE_DESCRIPTOR_READY;
    bits |= ENGINE_STATE_RESOURCE_READY | ENGINE_STATE_HW_READY;
    flags.fetch_or(bits, Ordering::AcqRel);
    true
}

fn runtime_engine_init(hot_path: u32, descriptor: DriverRuntimeInitDescriptor) -> bool {
    match hot_path {
        HOT_PATH_USB_KEYBOARD => usb_runtime_init(descriptor),
        HOT_PATH_HDMI_TEXT => {
            HDMI_CURSOR_ROW.store(0, Ordering::Release);
            HDMI_CURSOR_COL.store(0, Ordering::Release);
            true
        }
        HOT_PATH_GENET_NIC => GENET_RUNTIME_STATE.with_mut(|state| genet_runtime_init(state)),
        HOT_PATH_CYW43_WIFI => {
            CYW43_RUNTIME_STATE.with_mut(|state| cyw43_runtime_init(state, descriptor))
        }
        HOT_PATH_SDIO_HOST => SDIO_RUNTIME_STATE.with_mut(|state| sdio_runtime_init(state)),
        HOT_PATH_PCIE_ROOT => PCIE_RUNTIME_STATE.with_mut(|state| pcie_runtime_init(state)),
        _ => true,
    }
}

fn descriptor_resources_ready(descriptor: DriverRuntimeInitDescriptor, hot_path: u32) -> bool {
    let mmio_pages = descriptor.resource_pages_or_count(
        DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
        descriptor.mmio_page_count,
    );
    let dma_pages = descriptor
        .resource_pages_or_count(DRIVER_RUNTIME_RESOURCE_KIND_DMA, descriptor.dma_page_count);
    let shared_pages = descriptor.resource_pages_or_count(
        DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
        descriptor.shared_page_count,
    );
    match hot_path {
        HOT_PATH_USB_KEYBOARD => {
            mmio_pages >= USB_REQUIRED_MMIO_PAGES
                && dma_pages >= USB_REQUIRED_DMA_PAGES
                && shared_pages >= USB_REQUIRED_SHARED_PAGES
                && descriptor.has_resource_range_at(
                    DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
                    DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI,
                    DRIVER_TASK_DEVICE_MMIO_VADDR as u64,
                    USB_REQUIRED_MMIO_PAGES,
                )
                && descriptor.has_resource_range_at_with_flags(
                    DRIVER_RUNTIME_RESOURCE_KIND_DMA,
                    DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
                    DRIVER_TASK_DMA_BUFFER_VADDR as u64,
                    USB_REQUIRED_DMA_PAGES,
                    DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS,
                )
                && descriptor.has_pointer_free_bus_link(
                    HOT_PATH_PCIE_ROOT,
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE,
                )
        }
        HOT_PATH_HDMI_TEXT => {
            mmio_pages >= HDMI_REQUIRED_MMIO_PAGES
                && dma_pages >= HDMI_REQUIRED_DMA_PAGES
                && shared_pages >= HDMI_REQUIRED_SHARED_PAGES
                && descriptor.hdmi_ready()
                && descriptor.has_resource_range_at_with_flags(
                    DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER,
                    DRIVER_RUNTIME_RESOURCE_TAG_HDMI_FRAMEBUFFER,
                    DRIVER_RUNTIME_FRAMEBUFFER_VADDR,
                    1,
                    DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS,
                )
        }
        HOT_PATH_GENET_NIC => {
            mmio_pages >= GENET_REQUIRED_MMIO_PAGES
                && dma_pages >= GENET_REQUIRED_DMA_PAGES
                && shared_pages >= GENET_REQUIRED_SHARED_PAGES
                && descriptor.has_resource_range_at(
                    DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
                    DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
                    DRIVER_TASK_DEVICE_MMIO_VADDR as u64,
                    GENET_REQUIRED_MMIO_PAGES,
                )
                && descriptor.has_resource_range_at_with_flags(
                    DRIVER_RUNTIME_RESOURCE_KIND_DMA,
                    DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
                    DRIVER_TASK_DMA_BUFFER_VADDR as u64,
                    GENET_REQUIRED_DMA_PAGES,
                    DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS,
                )
        }
        HOT_PATH_CYW43_WIFI => {
            mmio_pages >= CYW43_REQUIRED_MMIO_PAGES
                && dma_pages >= CYW43_REQUIRED_DMA_PAGES
                && shared_pages >= CYW43_REQUIRED_SHARED_PAGES
                && descriptor.has_resource_range_at(
                    DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
                    DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
                    DRIVER_TASK_DEVICE_MMIO_VADDR as u64,
                    CYW43_REQUIRED_MMIO_PAGES,
                )
                && descriptor.has_resource_range_at(
                    DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                    DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL,
                    DRIVER_TASK_SHARED_BUFFER_VADDR as u64,
                    CYW43_REQUIRED_SHARED_PAGES,
                )
                && descriptor.has_pointer_free_bus_link(
                    HOT_PATH_SDIO_HOST,
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
                )
        }
        HOT_PATH_SDIO_HOST => {
            mmio_pages >= SDIO_REQUIRED_MMIO_PAGES
                && dma_pages >= SDIO_REQUIRED_DMA_PAGES
                && shared_pages >= SDIO_REQUIRED_SHARED_PAGES
                && descriptor.has_resource_range_at(
                    DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
                    DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
                    DRIVER_TASK_DEVICE_MMIO_VADDR as u64,
                    SDIO_REQUIRED_MMIO_PAGES,
                )
                && descriptor.has_resource_range_at_with_flags(
                    DRIVER_RUNTIME_RESOURCE_KIND_DMA,
                    DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
                    DRIVER_TASK_DMA_BUFFER_VADDR as u64,
                    SDIO_REQUIRED_DMA_PAGES,
                    DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS,
                )
        }
        HOT_PATH_PCIE_ROOT => {
            mmio_pages >= PCIE_REQUIRED_MMIO_PAGES
                && shared_pages >= PCIE_REQUIRED_SHARED_PAGES
                && descriptor.has_resource_range_at(
                    DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
                    DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST,
                    DRIVER_TASK_DEVICE_MMIO_VADDR as u64,
                    PCIE_REQUIRED_MMIO_PAGES,
                )
                && descriptor.has_resource_range_at(
                    DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                    DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL,
                    DRIVER_TASK_SHARED_BUFFER_VADDR as u64,
                    PCIE_REQUIRED_SHARED_PAGES,
                )
        }
        _ => false,
    }
}

fn engine_initialized(flags: &AtomicU32) -> bool {
    flags.load(Ordering::Acquire) & ENGINE_STATE_INITIALIZED != 0
}

fn runtime_resource_range(
    descriptor: DriverRuntimeInitDescriptor,
    kind: u16,
    tag: u32,
) -> Option<DriverRuntimeResourceRangeDescriptor> {
    let mut index = 0usize;
    while index < descriptor.resource_range_count as usize {
        let range = descriptor.resource_ranges[index];
        if range.kind == kind && range.tag == tag {
            return Some(range);
        }
        index += 1;
    }
    None
}

fn runtime_bus_addr(descriptor: DriverRuntimeInitDescriptor, paddr: u64) -> u64 {
    (paddr & descriptor.bus_alias_and) | descriptor.bus_alias_or
}

fn ring_slot(index: u16, slots: usize) -> usize {
    if slots == 0 {
        0
    } else {
        usize::from(index) % slots
    }
}

fn ring_distance(newer: u16, older: u16) -> u16 {
    newer.wrapping_sub(older)
}

const fn genet_rx_owned_len_status() -> u32 {
    ((GENET_RX_BUF_LENGTH as u32) << GENET_DMA_BUFLENGTH_SHIFT) | GENET_DMA_OWN
}

const fn genet_tx_len_status(len: usize) -> u32 {
    ((len as u32) << GENET_DMA_BUFLENGTH_SHIFT)
        | (GENET_DMA_DEFAULT_QTAG << GENET_DMA_TX_QTAG_SHIFT)
        | GENET_DMA_TX_APPEND_CRC
        | GENET_DMA_SOP
        | GENET_DMA_EOP
}

const fn genet_decode_rx_len(len_status: u32) -> usize {
    ((len_status >> GENET_DMA_BUFLENGTH_SHIFT) & GENET_DMA_BUFLENGTH_MASK) as usize
}

const fn genet_ring_end_addr(ring_descs: usize) -> u32 {
    let words = ring_descs.saturating_mul(GENET_DMA_DESC_SIZE) / 4;
    words.saturating_sub(1) as u32
}

const fn genet_ring_buffer_size(ring_descs: usize) -> u32 {
    ((ring_descs as u32) << GENET_DMA_RING_SIZE_SHIFT) | GENET_RX_BUF_LENGTH as u32
}

const fn genet_dma_fc_thresh_value(ring_descs: usize) -> u32 {
    (GENET_DMA_FC_THRESH_LO << 16) | ((ring_descs as u32) >> 4)
}

const fn genet_rx_desc_offset(slot: usize) -> Option<usize> {
    match slot.checked_mul(GENET_DMA_DESC_SIZE) {
        Some(offset) => GENET_RX_OFF.checked_add(offset),
        None => None,
    }
}

const fn genet_tx_desc_offset(slot: usize) -> Option<usize> {
    match slot.checked_mul(GENET_DMA_DESC_SIZE) {
        Some(offset) => GENET_TX_OFF.checked_add(offset),
        None => None,
    }
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

fn usb_runtime_init(descriptor: DriverRuntimeInitDescriptor) -> bool {
    let ok = USB_RUNTIME_STATE.with_mut(|state| {
        state.reset();
        let initialized = usb_runtime_init_hw(descriptor, state);
        state.initialized = initialized;
        initialized
    });
    if ok {
        USB_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_HW_READY, Ordering::AcqRel);
    }
    ok
}

fn genet_runtime_init(state: &mut GenetRuntimeState) -> bool {
    state.reset();
    let descriptor = RUNTIME_DESCRIPTOR.load();
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return false;
    };
    if dma_range.page_count < GENET_REQUIRED_DMA_PAGES {
        return false;
    }
    let initialized = genet_runtime_init_hw(descriptor, state);
    state.initialized = initialized;
    initialized
}

fn service_genet_runtime(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !engine_initialized(&GENET_RUNTIME_FLAGS) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    if command.frame.len == 0 {
        let read = GENET_RUNTIME_STATE.with_mut(genet_runtime_poll_rx);
        return if read == 0 {
            DriverTaskCompletionRecord::idle(command.sequence)
        } else {
            GENET_RX_COUNT.fetch_add(1, Ordering::AcqRel);
            GENET_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_RX_PROGRESS, Ordering::AcqRel);
            DriverTaskCompletionRecord::frame_ready(command.sequence, read as u16)
        };
    }
    if !command.frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let written =
        GENET_RUNTIME_STATE.with_mut(|state| genet_runtime_submit_tx(state, command.frame));
    if written == 0 {
        DriverTaskCompletionRecord::idle(command.sequence)
    } else {
        GENET_TX_COUNT.fetch_add(1, Ordering::AcqRel);
        GENET_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
        DriverTaskCompletionRecord::progress(command.sequence, written as u32)
    }
}

fn cyw43_runtime_init(
    state: &mut Cyw43RuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
) -> bool {
    state.reset();
    state.bus_link_ready = descriptor.has_pointer_free_bus_link(
        HOT_PATH_SDIO_HOST,
        DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
    );
    state.initialized = state.bus_link_ready
        && runtime_resource_range(
            descriptor,
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
        )
        .is_some()
        && runtime_resource_range(
            descriptor,
            DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
            DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL,
        )
        .is_some();
    if state.initialized {
        state.transport_ready = cyw43_transport_init(state);
        state.initialized = state.transport_ready;
    }
    state.initialized
}

fn service_cyw43_runtime(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !engine_initialized(&CYW43_RUNTIME_FLAGS) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    }
    if command.aux0 == DRIVER_RUNTIME_CYW43_COMMAND_AUX {
        return service_cyw43_descriptor_command(command);
    }
    if command.frame.len == 0 {
        let produced = CYW43_RUNTIME_STATE.with_mut(cyw43_runtime_poll_rx);
        return if produced == 0 {
            DriverTaskCompletionRecord::idle(command.sequence)
        } else {
            CYW43_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_RX_PROGRESS, Ordering::AcqRel);
            DriverTaskCompletionRecord::frame_ready(command.sequence, produced as u16)
        };
    }
    if !command.frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let encoded = CYW43_RUNTIME_STATE.with_mut(|state| cyw43_stage_sdpcm_tx(state, command.frame));
    if encoded == 0 {
        DriverTaskCompletionRecord::idle(command.sequence)
    } else {
        CYW43_TX_COUNT.fetch_add(1, Ordering::AcqRel);
        CYW43_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
        DriverTaskCompletionRecord::progress(command.sequence, encoded as u32)
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
        let produced = USB_RUNTIME_STATE.with_mut(usb_runtime_poll_keyboard);
        return if produced == 0 {
            DriverTaskCompletionRecord::idle(command.sequence)
        } else {
            USB_RUNTIME_STATE.with_mut(|state| {
                state.reports = state.reports.saturating_add(1);
            });
            USB_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_RX_PROGRESS, Ordering::AcqRel);
            DriverTaskCompletionRecord::frame_ready(command.sequence, produced as u16)
        };
    }
    #[cfg(not(test))]
    {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    #[cfg(test)]
    {
        if !command.frame.in_ring_payload() || command.frame.len as usize != USB_BOOT_REPORT_BYTES {
            return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
        }
        let report = read_frame_prefix::<USB_BOOT_REPORT_BYTES>(command.frame);
        let produced =
            USB_RUNTIME_STATE.with_mut(|state| usb_keyboard_report_bytes_to_frame(state, report));
        if produced == 0 {
            DriverTaskCompletionRecord::idle(command.sequence)
        } else {
            USB_RUNTIME_STATE.with_mut(|state| {
                state.reports = state.reports.saturating_add(1);
            });
            USB_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_RX_PROGRESS, Ordering::AcqRel);
            DriverTaskCompletionRecord::frame_ready(command.sequence, produced as u16)
        }
    }
}

fn service_hdmi_text(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    if let Some(completion) = service_engine_init(command) {
        return completion;
    }
    if !engine_initialized(&HDMI_RUNTIME_FLAGS) {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
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
        HDMI_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
        DriverTaskCompletionRecord::progress(command.sequence, rendered as u32)
    }
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
    if command.frame.len as usize == core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>() {
        return service_sdio_descriptor_command(command);
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
    SDIO_RUNTIME_STATE.with_mut(|state| {
        state.commands = state.commands.saturating_add(1);
        state.last_response = response0;
    });
    if data_len != 0 && flags & DRIVER_RUNTIME_SDIO_FLAG_WRITE == 0 {
        DriverTaskCompletionRecord::frame_ready(command.sequence, command.frame.len)
    } else if data_len != 0 {
        DriverTaskCompletionRecord::progress(command.sequence, data_len)
    } else {
        DriverTaskCompletionRecord::progress(command.sequence, response0)
    }
}

fn service_sdio_descriptor_command(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    let Some(desc) = read_sdio_command_descriptor(command.frame) else {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    };
    if !desc.valid() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let mut frame = DriverFrameDescriptor {
        offset: u32::from(desc.data_offset),
        len: desc.len,
        flags: 0,
    };
    if desc.block_count != 0 {
        let bytes = u32::from(desc.block_count).saturating_mul(u32::from(desc.block_size));
        if bytes > u32::from(u16::MAX) {
            return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
        }
        frame.len = bytes as u16;
    }
    if frame.len != 0 && !frame.in_ring_payload() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let flags = sdio_descriptor_response_flags(desc.response_kind)
        | match desc.op {
            DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE => {
                DRIVER_RUNTIME_SDIO_FLAG_DATA | DRIVER_RUNTIME_SDIO_FLAG_WRITE
            }
            DRIVER_RUNTIME_SDIO_OP_CMD53_READ => DRIVER_RUNTIME_SDIO_FLAG_DATA,
            _ => 0,
        };
    let (cmd, arg) = match desc.op {
        DRIVER_RUNTIME_SDIO_OP_CMD52_READ => {
            (52, sdio_cmd52_arg(false, desc.function, desc.addr, 0))
        }
        DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE => {
            let value = if desc.len == 0 {
                0
            } else {
                read_ring_byte(usize::from(desc.data_offset))
            };
            (52, sdio_cmd52_arg(true, desc.function, desc.addr, value))
        }
        DRIVER_RUNTIME_SDIO_OP_CMD53_READ | DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE => (
            53,
            sdio_cmd53_arg(
                desc.op == DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE,
                desc.function,
                desc.addr,
                desc.flags & DriverRuntimeSdioCommandDescriptor::FLAG_INCREMENT != 0,
                desc.block_count,
                desc.len,
            ),
        ),
        DRIVER_RUNTIME_SDIO_OP_POLL_IRQ => (52, sdio_cmd52_arg(false, 0, 0x05, 0)),
        _ => return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND),
    };
    let block_size = if desc.block_count == 0 {
        frame.len.max(1)
    } else {
        desc.block_size
    };
    let block_count = desc
        .block_count
        .max(if flags & DRIVER_RUNTIME_SDIO_FLAG_DATA != 0 {
            1
        } else {
            0
        });
    let Some(response0) = sdio_execute_transfer(cmd, arg, flags, frame, block_size, block_count)
    else {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE);
    };
    SDIO_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
    SDIO_CMD_COUNT.fetch_add(1, Ordering::AcqRel);
    SDIO_RUNTIME_STATE.with_mut(|state| {
        state.commands = state.commands.saturating_add(1);
        state.last_response = response0;
    });
    match desc.op {
        DRIVER_RUNTIME_SDIO_OP_CMD52_READ | DRIVER_RUNTIME_SDIO_OP_POLL_IRQ => {
            write_ring_byte(usize::from(desc.data_offset), (response0 & 0xff) as u8);
            DriverTaskCompletionRecord::frame_ready(command.sequence, 1)
        }
        DRIVER_RUNTIME_SDIO_OP_CMD53_READ => {
            DriverTaskCompletionRecord::frame_ready(command.sequence, frame.len)
        }
        DRIVER_RUNTIME_SDIO_OP_CMD52_WRITE | DRIVER_RUNTIME_SDIO_OP_CMD53_WRITE => {
            DriverTaskCompletionRecord::progress(command.sequence, u32::from(frame.len))
        }
        _ => DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND),
    }
}

fn read_sdio_command_descriptor(
    frame: DriverFrameDescriptor,
) -> Option<DriverRuntimeSdioCommandDescriptor> {
    if !frame.in_ring_payload()
        || frame.len as usize != core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>()
    {
        return None;
    }
    let offset = frame.offset as usize;
    Some(DriverRuntimeSdioCommandDescriptor {
        op: read_ring_u16(offset),
        function: read_ring_byte(offset + 2),
        response_kind: read_ring_byte(offset + 3),
        addr: read_ring_u32(offset + 4),
        data_offset: read_ring_u16(offset + 8),
        len: read_ring_u16(offset + 10),
        block_size: read_ring_u16(offset + 12),
        block_count: read_ring_u16(offset + 14),
        flags: read_ring_u16(offset + 16),
        reserved: read_ring_u16(offset + 18),
        timeout_us: read_ring_u32(offset + 20),
    })
}

fn read_cyw43_command_descriptor(
    frame: DriverFrameDescriptor,
) -> Option<DriverRuntimeCyw43CommandDescriptor> {
    if !frame.in_ring_payload()
        || frame.len as usize != core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>()
    {
        return None;
    }
    let offset = frame.offset as usize;
    Some(DriverRuntimeCyw43CommandDescriptor {
        op: read_ring_u16(offset),
        flags: read_ring_u16(offset + 2),
        target_addr: read_ring_u32(offset + 4),
        payload_offset: read_ring_u16(offset + 8),
        payload_len: read_ring_u16(offset + 10),
        total_len: read_ring_u32(offset + 12),
        arg0: read_ring_u32(offset + 16),
        arg1: read_ring_u32(offset + 20),
        reserved: read_ring_u32(offset + 24),
    })
}

fn service_cyw43_descriptor_command(
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord {
    let Some(desc) = read_cyw43_command_descriptor(command.frame) else {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    };
    if !desc.valid() {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    let result = CYW43_RUNTIME_STATE.with_mut(|state| match desc.op {
        DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT => {
            if cyw43_transport_init(state) {
                1
            } else {
                0
            }
        }
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK => {
            if cyw43_ram_payload_bounds(desc.target_addr, desc.payload_len)
                && cyw43_backplane_write_ring(
                    state,
                    desc.target_addr,
                    usize::from(desc.payload_offset),
                    usize::from(desc.payload_len),
                )
            {
                state.firmware_uploaded = true;
                state.firmware_bytes = state
                    .firmware_bytes
                    .saturating_add(u32::from(desc.payload_len));
                u32::from(desc.payload_len)
            } else {
                0
            }
        }
        DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK => {
            if cyw43_ram_payload_bounds(desc.target_addr, desc.payload_len)
                && cyw43_backplane_write_ring(
                    state,
                    desc.target_addr,
                    usize::from(desc.payload_offset),
                    usize::from(desc.payload_len),
                )
            {
                state.nvram_uploaded = true;
                state.nvram_bytes = state
                    .nvram_bytes
                    .saturating_add(u32::from(desc.payload_len));
                u32::from(desc.payload_len)
            } else {
                0
            }
        }
        DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL => {
            if cyw43_ram_payload_bounds(desc.target_addr, 4)
                && cyw43_backplane_write_u32(state, desc.target_addr, desc.arg0)
            {
                4
            } else {
                0
            }
        }
        DRIVER_RUNTIME_CYW43_OP_RELEASE => {
            if cyw43_release_firmware(state, desc.arg0) {
                state.firmware_released = true;
                1
            } else {
                0
            }
        }
        DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME => {
            let frame = DriverFrameDescriptor {
                offset: u32::from(desc.payload_offset),
                len: desc.payload_len,
                flags: 0,
            };
            cyw43_submit_sdpcm_frame(state, frame, false) as u32
        }
        DRIVER_RUNTIME_CYW43_OP_ETH_TX => {
            let frame = DriverFrameDescriptor {
                offset: u32::from(desc.payload_offset),
                len: desc.payload_len,
                flags: 0,
            };
            cyw43_submit_sdpcm_frame(state, frame, true) as u32
        }
        DRIVER_RUNTIME_CYW43_OP_RX_POLL => cyw43_runtime_poll_rx(state) as u32,
        _ => 0,
    });
    if desc.op == DRIVER_RUNTIME_CYW43_OP_RX_POLL && result == 0 {
        DriverTaskCompletionRecord::idle(command.sequence)
    } else if result == 0 {
        DriverTaskCompletionRecord::fault(command.sequence, FAULT_DEVICE_UNAVAILABLE)
    } else if desc.op == DRIVER_RUNTIME_CYW43_OP_RX_POLL {
        DriverTaskCompletionRecord::frame_ready(command.sequence, result as u16)
    } else {
        DriverTaskCompletionRecord::progress(command.sequence, result)
    }
}

const fn sdio_descriptor_response_flags(response_kind: u8) -> u16 {
    match response_kind {
        DRIVER_RUNTIME_SDIO_RESP_NONE => DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE,
        DRIVER_RUNTIME_SDIO_RESP_OCR => DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR,
        DRIVER_RUNTIME_SDIO_RESP_SHORT => DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        DRIVER_RUNTIME_SDIO_RESP_SHORT_BUSY => DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY,
        DRIVER_RUNTIME_SDIO_RESP_LONG => DRIVER_RUNTIME_SDIO_FLAG_RESP_LONG,
        _ => 0,
    }
}

const fn sdio_cmd52_arg(write: bool, function: u8, addr: u32, value: u8) -> u32 {
    (if write { 1 << 31 } else { 0 })
        | ((function as u32 & 0x7) << 28)
        | ((addr & 0x1ffff) << 9)
        | value as u32
}

const fn sdio_cmd53_arg(
    write: bool,
    function: u8,
    addr: u32,
    increment: bool,
    block_count: u16,
    len: u16,
) -> u32 {
    let block_mode = block_count != 0;
    let count = if block_mode { block_count } else { len };
    (if write { 1 << 31 } else { 0 })
        | ((function as u32 & 0x7) << 28)
        | (if block_mode { 1 << 27 } else { 0 })
        | (if increment { 1 << 26 } else { 0 })
        | ((addr & 0x1ffff) << 9)
        | ((count as u32) & 0x1ff)
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
    sdio_execute_transfer(cmd, arg, flags, frame, frame.len.max(1), 1)
}

#[cfg(target_os = "none")]
fn sdio_execute_transfer(
    cmd: u16,
    arg: u32,
    flags: u16,
    frame: DriverFrameDescriptor,
    block_size: u16,
    block_count: u16,
) -> Option<u32> {
    let has_data = flags & DRIVER_RUNTIME_SDIO_FLAG_DATA != 0;
    let write = flags & DRIVER_RUNTIME_SDIO_FLAG_WRITE != 0;
    if !sdio_wait_inhibit_clear(has_data) {
        return None;
    }
    sdio_write32(SDHCI_INT_STATUS, SDHCI_INT_COMMAND_DATA_CLEAR_MASK);
    if has_data {
        sdio_write16(SDHCI_BLOCK_SIZE, block_size);
        sdio_write16(SDHCI_BLOCK_COUNT, block_count.max(1));
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

#[cfg(not(target_os = "none"))]
fn sdio_execute_transfer(
    _cmd: u16,
    _arg: u32,
    _flags: u16,
    _frame: DriverFrameDescriptor,
    _block_size: u16,
    _block_count: u16,
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
    if offset & 0x3 != 0
        || offset
            .checked_add(PCIE_MMIO_ACCESS_BYTES)
            .is_none_or(|end| end > mmio_limit)
    {
        return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
    }
    match op {
        DRIVER_RUNTIME_PCIE_OP_PORT_READ => {
            if command.frame.len != 0 {
                return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
            }
            let value = pcie_read32(offset);
            PCIE_OP_COUNT.fetch_add(1, Ordering::AcqRel);
            PCIE_RUNTIME_STATE.with_mut(|state| {
                state.op_count = state.op_count.saturating_add(1);
            });
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
            PCIE_RUNTIME_STATE.with_mut(|state| {
                state.op_count = state.op_count.saturating_add(1);
            });
            PCIE_RUNTIME_FLAGS.fetch_or(ENGINE_STATE_TX_PROGRESS, Ordering::AcqRel);
            DriverTaskCompletionRecord::progress(command.sequence, 1)
        }
        DRIVER_RUNTIME_PCIE_OP_POSTED_WRITE_FLUSH => {
            if command.frame.len != 0 {
                return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND);
            }
            let _ = pcie_read32(offset);
            PCIE_OP_COUNT.fetch_add(1, Ordering::AcqRel);
            PCIE_RUNTIME_STATE.with_mut(|state| {
                state.op_count = state.op_count.saturating_add(1);
            });
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

fn cyw43_transport_init(state: &mut Cyw43RuntimeState) -> bool {
    if !state.bus_link_ready {
        return false;
    }
    if !sdio_runtime_init_hw() {
        return false;
    }
    state.backplane_window_valid = false;
    if !cyw43_sdio_card_init() {
        return false;
    }
    if !cyw43_set_function_block_size(1, SDIO_FUNCTION1_BLOCK_SIZE)
        || !cyw43_set_function_block_size(2, SDIO_FUNCTION2_BLOCK_SIZE)
        || !cyw43_enable_sdio_function(SDIO_FUNC_ENABLE_1, SDIO_FUNC_READY_1)
        || !cyw43_sdio_cmd52_write(0, SDIO_CCCR_IF, SDIO_BUS_WIDTH_4BIT)
        || !cyw43_backplane_bringup(state)
    {
        return false;
    }
    state.transport_ready = true;
    true
}

fn cyw43_sdio_card_init() -> bool {
    #[cfg(not(target_os = "none"))]
    {
        return true;
    }
    #[cfg(target_os = "none")]
    {
        let empty = DriverFrameDescriptor::empty();
        if sdio_execute_transfer(0, 0, DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE, empty, 1, 0).is_none() {
            return false;
        }
        let mut ocr = 0u32;
        for _ in 0..SDHCI_INIT_SPINS {
            let Some(response) =
                sdio_execute_transfer(SDIO_CMD5, 0, DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR, empty, 1, 0)
            else {
                return false;
            };
            ocr = response;
            if ocr & SDIO_OCR_3V2_3V4 != 0 {
                break;
            }
            core::hint::spin_loop();
        }
        if ocr & SDIO_OCR_3V2_3V4 == 0 {
            return false;
        }
        let desired_ocr = ocr & SDIO_OCR_3V2_3V4;
        for _ in 0..SDHCI_INIT_SPINS {
            let Some(response) = sdio_execute_transfer(
                SDIO_CMD5,
                desired_ocr,
                DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR,
                empty,
                1,
                0,
            ) else {
                return false;
            };
            if response & SDIO_R4_READY != 0 {
                ocr = response;
                break;
            }
            core::hint::spin_loop();
        }
        if ocr & SDIO_R4_READY == 0 {
            return false;
        }
        let Some(rca_response) = sdio_execute_transfer(
            SDIO_CMD3,
            0,
            DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
            empty,
            1,
            0,
        ) else {
            return false;
        };
        let rca = rca_response & 0xffff_0000;
        rca != 0
            && sdio_execute_transfer(
                SDIO_CMD7,
                rca,
                DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY,
                empty,
                1,
                0,
            )
            .is_some()
    }
}

fn cyw43_set_function_block_size(function: u8, size: u16) -> bool {
    let base = SDIO_CCCR_FBR_BASE.saturating_mul(u32::from(function));
    cyw43_sdio_cmd52_write(0, base + SDIO_FBR_BLKSIZE, (size & 0xff) as u8)
        && cyw43_sdio_cmd52_write(0, base + SDIO_FBR_BLKSIZE + 1, (size >> 8) as u8)
}

fn cyw43_enable_sdio_function(enable_bit: u8, ready_bit: u8) -> bool {
    #[cfg(not(target_os = "none"))]
    {
        let _ = enable_bit;
        let _ = ready_bit;
        return true;
    }
    #[cfg(target_os = "none")]
    {
        let current = cyw43_sdio_cmd52_read(0, SDIO_CCCR_IOEX).unwrap_or(0);
        let desired = current | enable_bit;
        if !cyw43_sdio_cmd52_write(0, SDIO_CCCR_IOEX, desired) {
            return false;
        }
        for _ in 0..SDHCI_INIT_SPINS {
            if cyw43_sdio_cmd52_read(0, SDIO_CCCR_IORX).unwrap_or(0) & ready_bit != 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }
}

fn cyw43_backplane_bringup(state: &mut Cyw43RuntimeState) -> bool {
    cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_CHIPCLKCSR, SBSDIO_ALP_AVAIL_REQ)
        && cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_WAKEUPCTRL, SBSDIO_WAKE_TILL_HT_AVAIL)
        && cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_SLEEPCSR, SBSDIO_FUNC1_SLEEPCSR_KSO_EN)
        && cyw43_sdio_cmd52_write(1, SBSDIO_WATERMARK, CY_43455_F2_WATERMARK)
        && cyw43_sdio_cmd52_write(1, SBSDIO_DEVICE_CTL, SBSDIO_DEVCTL_F2WM_ENAB)
        && cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_MESBUSYCTRL, CY_43455_MESBUSYCTRL)
        && cyw43_backplane_write_u32(
            state,
            CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET,
            u32::from(ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_PRERESET_IOCTRL),
        )
        && cyw43_backplane_write_u32(
            state,
            CYW43_ARMCR4_CORE_BASE + AI_RESETCTRL_OFFSET,
            u32::from(AI_RESETCTRL_BIT_RESET),
        )
}

fn cyw43_release_firmware(state: &mut Cyw43RuntimeState, reset_vector: u32) -> bool {
    if !state.transport_ready || !state.firmware_uploaded || !state.nvram_uploaded {
        return false;
    }
    if reset_vector != 0
        && !cyw43_backplane_write_u32(state, CYW43_FIRMWARE_RESET_VECTOR_ADDR, reset_vector)
    {
        return false;
    }
    if !cyw43_backplane_write_u32(
        state,
        CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET,
        u32::from(AI_CORE_POSTRESET_IOCTRL),
    ) || !cyw43_backplane_write_u32(state, CYW43_ARMCR4_CORE_BASE + AI_RESETCTRL_OFFSET, 0)
    {
        return false;
    }
    for _ in 0..SDHCI_INIT_SPINS {
        let chipclk = cyw43_sdio_cmd52_read(1, SBSDIO_FUNC1_CHIPCLKCSR).unwrap_or(0);
        if chipclk & (SBSDIO_HT_AVAIL | SBSDIO_ALP_AVAIL) != 0 {
            break;
        }
        let _ = cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_CHIPCLKCSR, SBSDIO_HT_AVAIL_REQ);
        core::hint::spin_loop();
    }
    cyw43_enable_sdio_function(SDIO_FUNC_ENABLE_2, SDIO_FUNC_READY_2)
        && cyw43_backplane_write_u32(
            state,
            CYW43_SDIO_CORE_BASE + SDPCMD_REG_HOSTINTMASK,
            HOSTINTMASK,
        )
        && cyw43_backplane_write_u32(
            state,
            CYW43_SDIO_CORE_BASE + SDPCMD_REG_FUNCTIONINTMASK,
            FUNCTIONINTMASK,
        )
        && cyw43_backplane_read_u32(state, CYW43_SDIO_CORE_BASE + SDIO_CORECONTROL)
            .is_some_and(|value| value & CC_F2RDY != 0 || cfg!(not(target_os = "none")))
}

fn cyw43_sdio_cmd52_read(function: u8, addr: u32) -> Option<u8> {
    sdio_execute_transfer(
        SDIO_CMD52,
        sdio_cmd52_arg(false, function, addr, 0),
        DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        DriverFrameDescriptor::empty(),
        1,
        0,
    )
    .map(|response| (response & 0xff) as u8)
}

fn cyw43_sdio_cmd52_write(function: u8, addr: u32, value: u8) -> bool {
    sdio_execute_transfer(
        SDIO_CMD52,
        sdio_cmd52_arg(true, function, addr, value),
        DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        DriverFrameDescriptor::empty(),
        1,
        0,
    )
    .is_some()
}

fn cyw43_backplane_set_window(state: &mut Cyw43RuntimeState, addr: u32) -> bool {
    let window = addr & BACKPLANE_WINDOW_MASK;
    if state.backplane_window_valid && state.backplane_window == window {
        return true;
    }
    let low = (window & 0xff) as u8;
    let mid = ((window >> 8) & 0xff) as u8;
    let high = ((window >> 16) & 0xff) as u8;
    if cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_SBADDRLOW, low)
        && cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_SBADDRMID, mid)
        && cyw43_sdio_cmd52_write(1, SBSDIO_FUNC1_SBADDRHIGH, high)
    {
        state.backplane_window = window;
        state.backplane_window_valid = true;
        true
    } else {
        false
    }
}

const fn cyw43_backplane_function_addr(addr: u32) -> u32 {
    (addr & BACKPLANE_ADDRESS_MASK) | BACKPLANE_32BIT_FLAG
}

fn cyw43_backplane_write_ring(
    state: &mut Cyw43RuntimeState,
    addr: u32,
    ring_offset: usize,
    len: usize,
) -> bool {
    let mut done = 0usize;
    while done < len {
        let Some(chunk_addr) = addr.checked_add(done as u32) else {
            return false;
        };
        if !cyw43_backplane_set_window(state, chunk_addr) {
            return false;
        }
        let chunk_len = (len - done).min(SDIO_FUNCTION2_BLOCK_SIZE as usize);
        let frame = DriverFrameDescriptor {
            offset: (ring_offset + done) as u32,
            len: chunk_len as u16,
            flags: 0,
        };
        if sdio_execute_transfer(
            SDIO_CMD53,
            sdio_cmd53_arg(
                true,
                1,
                cyw43_backplane_function_addr(chunk_addr),
                true,
                0,
                chunk_len as u16,
            ),
            DRIVER_RUNTIME_SDIO_FLAG_DATA
                | DRIVER_RUNTIME_SDIO_FLAG_WRITE
                | DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
            frame,
            chunk_len as u16,
            1,
        )
        .is_none()
        {
            return false;
        }
        done += chunk_len;
    }
    true
}

fn cyw43_backplane_write_u32(state: &mut Cyw43RuntimeState, addr: u32, value: u32) -> bool {
    write_ring_u32(DRIVER_TASK_RING_FRAME_OFFSET, value);
    cyw43_backplane_write_ring(state, addr, DRIVER_TASK_RING_FRAME_OFFSET, 4)
}

fn cyw43_backplane_read_u32(state: &mut Cyw43RuntimeState, addr: u32) -> Option<u32> {
    if !cyw43_backplane_set_window(state, addr) {
        return None;
    }
    let frame = DriverFrameDescriptor {
        offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
        len: 4,
        flags: 0,
    };
    sdio_execute_transfer(
        SDIO_CMD53,
        sdio_cmd53_arg(false, 1, cyw43_backplane_function_addr(addr), true, 0, 4),
        DRIVER_RUNTIME_SDIO_FLAG_DATA | DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        frame,
        4,
        1,
    )?;
    Some(read_ring_u32(DRIVER_TASK_RING_FRAME_OFFSET))
}

fn cyw43_stage_sdpcm_tx(state: &mut Cyw43RuntimeState, frame: DriverFrameDescriptor) -> usize {
    cyw43_submit_sdpcm_frame(state, frame, true)
}

fn cyw43_ram_payload_bounds(addr: u32, len: u16) -> bool {
    let Some(end) = addr.checked_add(u32::from(len)) else {
        return false;
    };
    addr >= CYW43_RAM_BASE_4345
        && end <= CYW43_RAM_BASE_4345.saturating_add(CYW43_RAM_SIZE_4345_PI4)
}

fn cyw43_submit_sdpcm_frame(
    state: &mut Cyw43RuntimeState,
    frame: DriverFrameDescriptor,
    data_frame: bool,
) -> usize {
    if !state.initialized || !state.transport_ready || !state.firmware_released {
        return 0;
    }
    let payload_len = frame.len as usize;
    let bdc_len = if data_frame {
        CYW43_BDC_HEADER_BYTES
    } else {
        0
    };
    let header_len = if data_frame {
        CYW43_BUS_HEADER_BYTES
    } else {
        CYW43_SDPCM_HEADER_BYTES
    };
    if payload_len == 0 || payload_len > MAX_DRIVER_TASK_FRAME_BYTES.saturating_sub(header_len) {
        return 0;
    }
    let total_len = payload_len + header_len;
    let seq = state.sdpcm_seq;
    state.sdpcm_seq = state.sdpcm_seq.wrapping_add(1);
    state.tx_frames = state.tx_frames.saturating_add(1);
    write_sdpcm_header(
        DRIVER_TASK_RING_FRAME_OFFSET,
        total_len,
        CYW43_SDPCM_HEADER_BYTES + bdc_len,
        seq,
        if data_frame { 2 } else { 0 },
    );
    if data_frame {
        for index in 0..CYW43_BDC_HEADER_BYTES {
            write_ring_byte(
                DRIVER_TASK_RING_FRAME_OFFSET + CYW43_SDPCM_HEADER_BYTES + index,
                0,
            );
        }
    }
    for index in 0..payload_len {
        let byte = read_ring_byte(frame.offset as usize + index);
        write_ring_byte(
            DRIVER_TASK_RING_FRAME_OFFSET + CYW43_SDPCM_HEADER_BYTES + bdc_len + index,
            byte,
        );
    }
    let request_len = align4(total_len);
    let tx_frame = DriverFrameDescriptor {
        offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
        len: request_len as u16,
        flags: 0,
    };
    if sdio_execute_transfer(
        SDIO_CMD53,
        sdio_cmd53_arg(true, 2, BACKPLANE_32BIT_FLAG, false, 0, request_len as u16),
        DRIVER_RUNTIME_SDIO_FLAG_DATA
            | DRIVER_RUNTIME_SDIO_FLAG_WRITE
            | DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        tx_frame,
        request_len as u16,
        1,
    )
    .is_none()
    {
        return 0;
    }
    total_len
}

fn cyw43_runtime_poll_rx(state: &mut Cyw43RuntimeState) -> usize {
    if !state.initialized || !state.transport_ready || !state.firmware_released {
        return 0;
    }
    let lo = cyw43_sdio_cmd52_read(1, SBSDIO_FUNC1_RFRAMEBCLO).unwrap_or(0);
    let hi = cyw43_sdio_cmd52_read(1, SBSDIO_FUNC1_RFRAMEBCHI).unwrap_or(0);
    let frame_len = usize::from(lo) | (usize::from(hi) << 8);
    if frame_len < CYW43_SDPCM_HEADER_BYTES || frame_len > MAX_DRIVER_TASK_FRAME_BYTES {
        return 0;
    }
    let request_len = align4(frame_len);
    let rx_frame = DriverFrameDescriptor {
        offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
        len: request_len as u16,
        flags: 0,
    };
    if sdio_execute_transfer(
        SDIO_CMD53,
        sdio_cmd53_arg(false, 2, BACKPLANE_32BIT_FLAG, false, 0, request_len as u16),
        DRIVER_RUNTIME_SDIO_FLAG_DATA | DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        rx_frame,
        request_len as u16,
        1,
    )
    .is_none()
    {
        return 0;
    }
    let Some((payload_offset, payload_len)) = cyw43_rx_payload_bounds(frame_len) else {
        return 0;
    };
    for index in 0..payload_len {
        let byte = read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + payload_offset + index);
        write_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + index, byte);
    }
    state.rx_frames = state.rx_frames.saturating_add(1);
    payload_len
}

fn cyw43_rx_payload_bounds(frame_len: usize) -> Option<(usize, usize)> {
    if frame_len < CYW43_SDPCM_HEADER_BYTES {
        return None;
    }
    let len = u16::from_le_bytes([
        read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET),
        read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + 1),
    ]) as usize;
    let len_inv = u16::from_le_bytes([
        read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + 2),
        read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + 3),
    ]);
    if len == 0 || len > frame_len || len_inv != !(len as u16) {
        return None;
    }
    let channel = read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + 5) & 0x0f;
    let data_offset = usize::from(read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + 7));
    if channel != 2 || data_offset < CYW43_SDPCM_HEADER_BYTES || data_offset >= len {
        return None;
    }
    let payload_offset = data_offset.checked_add(CYW43_BDC_HEADER_BYTES)?;
    if payload_offset > len {
        return None;
    }
    Some((payload_offset, len - payload_offset))
}

fn write_sdpcm_header(offset: usize, total_len: usize, data_offset: usize, seq: u8, channel: u8) {
    let total = total_len as u16;
    write_ring_byte(offset, (total & 0xff) as u8);
    write_ring_byte(offset + 1, (total >> 8) as u8);
    let inv = !total;
    write_ring_byte(offset + 2, (inv & 0xff) as u8);
    write_ring_byte(offset + 3, (inv >> 8) as u8);
    write_ring_byte(offset + 4, seq);
    write_ring_byte(offset + 5, channel & 0x0f);
    write_ring_byte(offset + 6, 0);
    write_ring_byte(offset + 7, data_offset as u8);
    for index in 8..CYW43_SDPCM_HEADER_BYTES {
        write_ring_byte(offset + index, 0);
    }
}

const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn genet_runtime_submit_tx(state: &mut GenetRuntimeState, frame: DriverFrameDescriptor) -> usize {
    if !state.initialized || frame.len == 0 || frame.len as usize > GENET_MAX_FRAME_LEN {
        state.tx_drops = state.tx_drops.saturating_add(1);
        return 0;
    }
    genet_runtime_poll_tx_completions(state);
    let in_flight = ring_distance(state.tx_prod_index, state.tx_cons_index) as usize;
    if in_flight >= GENET_HW_TOTAL_DESCS {
        state.tx_drops = state.tx_drops.saturating_add(1);
        return 0;
    }
    let descriptor = RUNTIME_DESCRIPTOR.load();
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return 0;
    };
    let slot = ring_slot(state.tx_prod_index, GENET_HW_TOTAL_DESCS);
    let dma_slot = GENET_HW_TOTAL_DESCS + slot;
    let Some(dma_vaddr) =
        (dma_range.vaddr as usize).checked_add(dma_slot * DRIVER_TASK_RING_PAGE_BYTES)
    else {
        return 0;
    };
    let Some(dma_paddr) = dma_range
        .paddr
        .checked_add((dma_slot as u64).saturating_mul(DRIVER_RUNTIME_RESOURCE_PAGE_BYTES))
    else {
        return 0;
    };
    for index in 0..frame.len as usize {
        write_dma_byte(
            dma_vaddr + index,
            read_ring_byte(frame.offset as usize + index),
        );
    }
    dma_store_barrier();
    genet_write_tx_desc(
        slot,
        runtime_bus_addr(descriptor, dma_paddr),
        genet_tx_len_status(frame.len as usize),
    );
    dma_store_barrier();
    state.tx_prod_index = state.tx_prod_index.wrapping_add(1);
    genet_write32(GENET_TDMA_PROD_INDEX, state.tx_prod_index as u32);
    state.tx_packets = state.tx_packets.saturating_add(1);
    frame.len as usize
}

fn genet_runtime_poll_rx(state: &mut GenetRuntimeState) -> usize {
    if !state.initialized {
        return 0;
    }
    genet_runtime_poll_tx_completions(state);
    let descriptor = RUNTIME_DESCRIPTOR.load();
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return 0;
    };
    for _ in 0..GENET_RX_DRAIN_BUDGET {
        let prod = genet_read32(GENET_RDMA_PROD_INDEX) as u16;
        if prod == state.rx_cons_index {
            return 0;
        }
        let slot = ring_slot(state.rx_cons_index, GENET_HW_TOTAL_DESCS);
        let desc = genet_read_rx_desc(slot);
        if desc.0 & GENET_DMA_OWN != 0 {
            return 0;
        }
        let length = genet_decode_rx_len(desc.0);
        let payload_len = length
            .saturating_sub(GENET_RX_BUF_OFFSET)
            .min(MAX_DRIVER_TASK_FRAME_BYTES);
        let dma_vaddr = dma_range.vaddr as usize + slot * DRIVER_TASK_RING_PAGE_BYTES;
        dma_load_barrier();
        for index in 0..payload_len {
            let byte = read_dma_byte(dma_vaddr + GENET_RX_BUF_OFFSET + index);
            write_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + index, byte);
        }
        genet_rearm_rx_slot(descriptor, dma_range, slot);
        state.rx_cons_index = state.rx_cons_index.wrapping_add(1);
        genet_write32(GENET_RDMA_CONS_INDEX, state.rx_cons_index as u32);
        if payload_len != 0 {
            state.rx_packets = state.rx_packets.saturating_add(1);
            return payload_len;
        }
    }
    0
}

fn genet_runtime_poll_tx_completions(state: &mut GenetRuntimeState) {
    let new_cons = genet_read32(GENET_TDMA_CONS_INDEX) as u16;
    let completed = ring_distance(new_cons, state.tx_cons_index) as usize;
    if completed != 0 {
        let reclaim = completed.min(GENET_TX_COMPLETION_RECLAIM_BUDGET);
        state.tx_cons_index = state.tx_cons_index.wrapping_add(reclaim as u16);
    }
}

fn genet_runtime_init_hw(
    descriptor: DriverRuntimeInitDescriptor,
    state: &mut GenetRuntimeState,
) -> bool {
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return false;
    };
    if dma_range.page_count < GENET_REQUIRED_DMA_PAGES {
        return false;
    }
    genet_hw_init_registers();
    genet_write_mac(GENET_DRIVER_TASK_MAC);
    if let Some((phy_addr, link_ready, speed)) = genet_configure_phy_link() {
        state.phy_addr = phy_addr;
        state.link_ready = link_ready;
        state.link_speed = speed;
    } else {
        state.link_speed = genet_current_umac_speed();
    }
    state.tx_cons_index = genet_read32(GENET_TDMA_CONS_INDEX) as u16;
    state.tx_prod_index = state.tx_cons_index;
    state.rx_cons_index = genet_read32(GENET_RDMA_PROD_INDEX) as u16;
    for slot in 0..GENET_HW_TOTAL_DESCS {
        genet_rearm_rx_slot(descriptor, dma_range, slot);
        genet_write_tx_desc(slot, 0, 0);
    }
    genet_enable_dma();
    let cmd = genet_read32(GENET_UMAC_CMD);
    genet_write32(GENET_UMAC_CMD, cmd | GENET_CMD_TX_EN | GENET_CMD_RX_EN);
    true
}

fn genet_hw_init_registers() {
    let mut flush = genet_read32(GENET_SYS_RBUF_FLUSH_CTRL);
    flush |= 1 << 1;
    genet_write32(GENET_SYS_RBUF_FLUSH_CTRL, flush);
    runtime_spin(10_000);
    genet_write32(GENET_SYS_RBUF_FLUSH_CTRL, flush & !(1 << 1));
    runtime_spin(10_000);
    genet_write32(GENET_UMAC_CMD, 0);
    genet_write32(GENET_UMAC_CMD, GENET_CMD_SW_RESET | GENET_CMD_LCL_LOOP_EN);
    runtime_spin(2_000);
    genet_write32(GENET_UMAC_CMD, 0);
    genet_write32(
        GENET_UMAC_MIB_CTRL,
        GENET_MIB_RESET_RX | GENET_MIB_RESET_TX | GENET_MIB_RESET_RUNT,
    );
    genet_write32(GENET_UMAC_MIB_CTRL, 0);
    genet_write32(GENET_UMAC_MAX_FRAME_LEN, GENET_RX_BUF_LENGTH as u32);
    genet_write32(
        GENET_RBUF_CTRL,
        genet_read32(GENET_RBUF_CTRL) | GENET_RBUF_ALIGN_2B,
    );
    genet_write32(GENET_RBUF_TBUF_SIZE_CTRL, 1);
    genet_write32(GENET_SYS_PORT_CTRL, GENET_PORT_MODE_EXT_GPHY);
    let mut oob = genet_read32(GENET_EXT_RGMII_OOB_CTRL);
    oob &= !GENET_OOB_DISABLE;
    oob |= GENET_RGMII_LINK | GENET_RGMII_MODE_EN | GENET_ID_MODE_DIS;
    genet_write32(GENET_EXT_RGMII_OOB_CTRL, oob);
    let mut cmd = genet_read32(GENET_UMAC_CMD);
    cmd &= !(GENET_CMD_SPEED_MASK << GENET_CMD_SPEED_SHIFT);
    cmd |= 2 << GENET_CMD_SPEED_SHIFT;
    genet_write32(GENET_UMAC_CMD, cmd);
    genet_disable_dma();
    genet_init_rx_ring();
    genet_init_tx_ring();
}

fn genet_configure_phy_link() -> Option<(u8, bool, u32)> {
    genet_write32(GENET_SYS_PORT_CTRL, GENET_PORT_MODE_EXT_GPHY);
    let mut oob = genet_read32(GENET_EXT_RGMII_OOB_CTRL);
    oob &= !GENET_OOB_DISABLE;
    oob |= GENET_RGMII_LINK | GENET_RGMII_MODE_EN | GENET_ID_MODE_DIS;
    genet_write32(GENET_EXT_RGMII_OOB_CTRL, oob);

    let phy_addr = genet_discover_phy_addr()?;
    if let Some(bmcr) = genet_mdio_read(phy_addr, GENET_MII_BMCR) {
        if (bmcr & GENET_MII_BMCR_ANENABLE) == 0 {
            let next = (bmcr | GENET_MII_BMCR_ANENABLE | GENET_MII_BMCR_ANRESTART)
                & !(GENET_MII_BMCR_SPEED100 | GENET_MII_BMCR_SPEED1000);
            let _ = genet_mdio_write(phy_addr, GENET_MII_BMCR, next);
        }
    }

    let mut link_ready = false;
    let mut resolved_speed = None;
    for _ in 0..GENET_PHY_LINK_POLL_TRIES {
        let _ = genet_mdio_read(phy_addr, GENET_MII_BMSR);
        if let Some(status) = genet_mdio_read(phy_addr, GENET_MII_BMSR) {
            link_ready = (status & GENET_MII_BMSR_LSTATUS) != 0;
            let autoneg_ready = (status & GENET_MII_BMSR_ANEGCOMPLETE) != 0;
            resolved_speed = genet_resolve_phy_speed(phy_addr).or(resolved_speed);
            if link_ready && (autoneg_ready || resolved_speed.is_some()) {
                break;
            }
        }
        runtime_spin(GENET_PHY_LINK_POLL_DELAY_SPINS);
    }

    let speed = resolved_speed
        .or_else(|| genet_mdio_read(phy_addr, GENET_MII_BMCR).map(genet_decode_bmcr_speed))
        .unwrap_or(if link_ready {
            2
        } else {
            genet_current_umac_speed()
        });
    genet_set_umac_speed(speed);
    Some((phy_addr, link_ready, speed))
}

fn genet_discover_phy_addr() -> Option<u8> {
    let mut addr = 0u8;
    while addr < 32 {
        let id1 = genet_mdio_read(addr, GENET_MII_PHYSID1);
        let id2 = genet_mdio_read(addr, GENET_MII_PHYSID2);
        if let (Some(id1), Some(id2)) = (id1, id2) {
            if id1 != 0 && id1 != u16::MAX && id2 != 0 && id2 != u16::MAX {
                return Some(addr);
            }
        }
        addr = addr.saturating_add(1);
    }
    None
}

fn genet_mdio_wait_idle() -> bool {
    for _ in 0..GENET_MDIO_POLL_TRIES {
        if (genet_read32(GENET_MDIO_CMD) & GENET_MDIO_START_BUSY) == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn genet_mdio_read(phy_addr: u8, reg: u8) -> Option<u16> {
    if !genet_mdio_wait_idle() {
        return None;
    }
    let mut cmd = GENET_MDIO_RD
        | ((u32::from(phy_addr) & GENET_MDIO_FIELD_MASK) << GENET_MDIO_PMD_SHIFT)
        | ((u32::from(reg) & GENET_MDIO_FIELD_MASK) << GENET_MDIO_REG_SHIFT);
    genet_write32(GENET_MDIO_CMD, cmd);
    cmd |= GENET_MDIO_START_BUSY;
    genet_write32(GENET_MDIO_CMD, cmd);
    if !genet_mdio_wait_idle() {
        return None;
    }
    let value = genet_read32(GENET_MDIO_CMD);
    if (value & GENET_MDIO_READ_FAIL) != 0 {
        return None;
    }
    Some((value & 0xffff) as u16)
}

fn genet_mdio_write(phy_addr: u8, reg: u8, value: u16) -> bool {
    if !genet_mdio_wait_idle() {
        return false;
    }
    let cmd = GENET_MDIO_WR
        | ((u32::from(phy_addr) & GENET_MDIO_FIELD_MASK) << GENET_MDIO_PMD_SHIFT)
        | ((u32::from(reg) & GENET_MDIO_FIELD_MASK) << GENET_MDIO_REG_SHIFT)
        | u32::from(value);
    genet_write32(GENET_MDIO_CMD, cmd | GENET_MDIO_START_BUSY);
    genet_mdio_wait_idle() && (genet_read32(GENET_MDIO_CMD) & GENET_MDIO_READ_FAIL) == 0
}

fn genet_resolve_phy_speed(phy_addr: u8) -> Option<u32> {
    let adv_1000 = genet_mdio_read(phy_addr, GENET_MII_CTRL1000)?;
    let lpa_1000 = genet_mdio_read(phy_addr, GENET_MII_STAT1000)?;
    if (adv_1000 & GENET_ADVERTISE_1000FULL) != 0 && (lpa_1000 & GENET_LPA_1000FULL) != 0 {
        return Some(2);
    }
    if (adv_1000 & GENET_ADVERTISE_1000HALF) != 0 && (lpa_1000 & GENET_LPA_1000HALF) != 0 {
        return Some(2);
    }

    let adv = genet_mdio_read(phy_addr, GENET_MII_ADVERTISE)?;
    let lpa = genet_mdio_read(phy_addr, GENET_MII_LPA)?;
    if (adv & GENET_LPA_100FULL) != 0 && (lpa & GENET_LPA_100FULL) != 0 {
        return Some(1);
    }
    if (adv & GENET_LPA_100HALF) != 0 && (lpa & GENET_LPA_100HALF) != 0 {
        return Some(1);
    }
    if (adv & GENET_LPA_10FULL) != 0 && (lpa & GENET_LPA_10FULL) != 0 {
        return Some(0);
    }
    if (adv & GENET_LPA_10HALF) != 0 && (lpa & GENET_LPA_10HALF) != 0 {
        return Some(0);
    }
    None
}

const fn genet_decode_bmcr_speed(bmcr: u16) -> u32 {
    let speed_100 = (bmcr & GENET_MII_BMCR_SPEED100) != 0;
    let speed_1000 = (bmcr & GENET_MII_BMCR_SPEED1000) != 0;
    match (speed_100, speed_1000) {
        (false, false) => 0,
        (true, false) => 1,
        (false, true) | (true, true) => 2,
    }
}

fn genet_current_umac_speed() -> u32 {
    (genet_read32(GENET_UMAC_CMD) >> GENET_CMD_SPEED_SHIFT) & GENET_CMD_SPEED_MASK
}

fn genet_set_umac_speed(speed: u32) {
    let mut cmd = genet_read32(GENET_UMAC_CMD);
    cmd &= !(GENET_CMD_SPEED_MASK << GENET_CMD_SPEED_SHIFT);
    cmd |= (speed & GENET_CMD_SPEED_MASK) << GENET_CMD_SPEED_SHIFT;
    genet_write32(GENET_UMAC_CMD, cmd);
}

fn genet_write_mac(mac: [u8; 6]) {
    let mac0 = (u32::from(mac[0]) << 24)
        | (u32::from(mac[1]) << 16)
        | (u32::from(mac[2]) << 8)
        | u32::from(mac[3]);
    let mac1 = (u32::from(mac[4]) << 8) | u32::from(mac[5]);
    genet_write32(GENET_UMAC_MAC0, mac0);
    genet_write32(GENET_UMAC_MAC1, mac1);
}

fn genet_disable_dma() {
    genet_write32(
        GENET_TDMA_REG_BASE + GENET_DMA_CTRL,
        genet_read32(GENET_TDMA_REG_BASE + GENET_DMA_CTRL) & !GENET_DMA_EN,
    );
    genet_write32(
        GENET_RDMA_REG_BASE + GENET_DMA_CTRL,
        genet_read32(GENET_RDMA_REG_BASE + GENET_DMA_CTRL) & !GENET_DMA_EN,
    );
    genet_write32(GENET_UMAC_TX_FLUSH, 1);
    runtime_spin(10_000);
    genet_write32(GENET_UMAC_TX_FLUSH, 0);
}

fn genet_enable_dma() {
    let ctrl = (1u32 << (GENET_DEFAULT_Q + GENET_DMA_RING_BUF_EN_SHIFT)) | GENET_DMA_EN;
    genet_write32(GENET_TDMA_REG_BASE + GENET_DMA_CTRL, ctrl);
    genet_write32(
        GENET_RDMA_REG_BASE + GENET_DMA_CTRL,
        genet_read32(GENET_RDMA_REG_BASE + GENET_DMA_CTRL) | ctrl,
    );
}

fn genet_init_rx_ring() {
    genet_write32(
        GENET_RDMA_REG_BASE + GENET_DMA_SCB_BURST_SIZE,
        GENET_DMA_MAX_BURST_LENGTH,
    );
    genet_write32(GENET_RDMA_RING_REG_BASE + GENET_DMA_START_ADDR, 0);
    genet_write32(GENET_RDMA_READ_PTR, 0);
    genet_write32(GENET_RDMA_WRITE_PTR, 0);
    genet_write32(
        GENET_RDMA_RING_REG_BASE + GENET_DMA_END_ADDR,
        genet_ring_end_addr(GENET_HW_TOTAL_DESCS),
    );
    genet_write32(GENET_RDMA_CONS_INDEX, genet_read32(GENET_RDMA_PROD_INDEX));
    genet_write32(
        GENET_RDMA_RING_REG_BASE + GENET_DMA_RING_BUF_SIZE,
        genet_ring_buffer_size(GENET_HW_TOTAL_DESCS),
    );
    genet_write32(
        GENET_RDMA_XON_XOFF_THRESH,
        genet_dma_fc_thresh_value(GENET_HW_TOTAL_DESCS),
    );
    genet_write32(
        GENET_RDMA_REG_BASE + GENET_DMA_RING_CFG,
        1u32 << GENET_DEFAULT_Q,
    );
}

fn genet_init_tx_ring() {
    genet_write32(
        GENET_TDMA_REG_BASE + GENET_DMA_SCB_BURST_SIZE,
        GENET_DMA_MAX_BURST_LENGTH,
    );
    genet_write32(GENET_TDMA_RING_REG_BASE + GENET_DMA_START_ADDR, 0);
    genet_write32(GENET_TDMA_READ_PTR, 0);
    genet_write32(GENET_TDMA_WRITE_PTR, 0);
    genet_write32(
        GENET_TDMA_RING_REG_BASE + GENET_DMA_END_ADDR,
        genet_ring_end_addr(GENET_HW_TOTAL_DESCS),
    );
    genet_write32(GENET_TDMA_PROD_INDEX, genet_read32(GENET_TDMA_CONS_INDEX));
    genet_write32(GENET_TDMA_RING_REG_BASE + GENET_DMA_MBUF_DONE_THRESH, 1);
    genet_write32(GENET_TDMA_FLOW_PERIOD, 0);
    genet_write32(
        GENET_TDMA_RING_REG_BASE + GENET_DMA_RING_BUF_SIZE,
        genet_ring_buffer_size(GENET_HW_TOTAL_DESCS),
    );
    genet_write32(
        GENET_TDMA_REG_BASE + GENET_DMA_RING_CFG,
        1u32 << GENET_DEFAULT_Q,
    );
}

fn genet_rearm_rx_slot(
    descriptor: DriverRuntimeInitDescriptor,
    dma_range: DriverRuntimeResourceRangeDescriptor,
    slot: usize,
) {
    let dma_paddr = dma_range
        .paddr
        .saturating_add((slot as u64).saturating_mul(DRIVER_RUNTIME_RESOURCE_PAGE_BYTES));
    genet_write_rx_desc(
        slot,
        runtime_bus_addr(descriptor, dma_paddr),
        genet_rx_owned_len_status(),
    );
}

fn genet_write_tx_desc(slot: usize, paddr: u64, len_status: u32) {
    if let Some(base) = genet_tx_desc_offset(slot) {
        genet_write32(base + 0x04, paddr as u32);
        genet_write32(base + 0x08, (paddr >> 32) as u32);
        genet_write32(base, len_status);
    }
}

fn genet_write_rx_desc(slot: usize, paddr: u64, len_status: u32) {
    if let Some(base) = genet_rx_desc_offset(slot) {
        genet_write32(base + 0x04, paddr as u32);
        genet_write32(base + 0x08, (paddr >> 32) as u32);
        genet_write32(base, len_status);
    }
}

fn genet_read_rx_desc(slot: usize) -> (u32, u32, u32) {
    if let Some(base) = genet_rx_desc_offset(slot) {
        (
            genet_read32(base),
            genet_read32(base + 0x04),
            genet_read32(base + 0x08),
        )
    } else {
        (0, 0, 0)
    }
}

fn sdio_runtime_init(state: &mut SdioRuntimeState) -> bool {
    state.reset();
    let ok = sdio_runtime_init_hw();
    state.initialized = ok;
    ok
}

fn pcie_runtime_init(state: &mut PcieRuntimeState) -> bool {
    state.reset();
    let ok = pcie_runtime_init_hw(state);
    state.initialized = ok;
    ok
}

fn usb_runtime_poll_keyboard(state: &mut UsbRuntimeState) -> usize {
    if !state.initialized || state.keyboard_slot == 0 || state.keyboard_endpoint_id == 0 {
        return 0;
    }
    let descriptor = RUNTIME_DESCRIPTOR.load();
    if !state.keyboard_report_queued && !xhci_queue_keyboard_interrupt_in(state, descriptor) {
        return 0;
    }
    let Some(event) = xhci_next_event(state, descriptor) else {
        return 0;
    };
    if xhci_trb_type(event.control) != XHCI_TRB_TYPE_TRANSFER_EVENT
        || xhci_slot_id(event.control) != state.keyboard_slot
        || xhci_endpoint_id(event.control) != state.keyboard_endpoint_id
    {
        return 0;
    }
    xhci_ack_event_dequeue(state, descriptor);
    let code = xhci_completion_code(event.status);
    if code != XHCI_COMPLETION_SUCCESS && code != XHCI_COMPLETION_SHORT_PACKET {
        state.keyboard_report_queued = false;
        return 0;
    }
    state.keyboard_report_queued = false;
    dma_load_barrier();
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return 0;
    };
    let report_vaddr = dma_range.vaddr as usize + XHCI_DMA_REPORT_BUFFER_OFFSET;
    let mut report = [0u8; USB_BOOT_REPORT_BYTES];
    for (index, byte) in report.iter_mut().enumerate() {
        *byte = read_dma_byte(report_vaddr + index);
    }
    usb_keyboard_report_bytes_to_frame(state, report)
}

fn usb_keyboard_report_bytes_to_frame(
    state: &mut UsbRuntimeState,
    report: [u8; USB_BOOT_REPORT_BYTES],
) -> usize {
    let mut produced = 0usize;
    for &code in report[2..].iter() {
        if code == 0 {
            continue;
        }
        if state.last_keys.contains(&code) {
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
    state.last_keys.copy_from_slice(&report[2..8]);
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

#[cfg(test)]
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

#[cfg(all(not(target_os = "none"), test))]
struct TestRing(UnsafeCell<[u8; DRIVER_TASK_RING_PAGE_BYTES]>);

#[cfg(all(not(target_os = "none"), test))]
// SAFETY: Runtime tests serialize all access through `test_guard`, so the
// process-local ring buffer is never concurrently mutated.
unsafe impl Sync for TestRing {}

#[cfg(all(not(target_os = "none"), test))]
static TEST_RING: TestRing = TestRing(UnsafeCell::new([0; DRIVER_TASK_RING_PAGE_BYTES]));

#[cfg(all(not(target_os = "none"), test))]
fn reset_test_ring() {
    // SAFETY: Runtime tests hold `test_guard` before resetting shared state.
    unsafe {
        (*TEST_RING.0.get()).fill(0);
    }
}

#[cfg(all(not(target_os = "none"), test))]
fn read_ring_byte(offset: usize) -> u8 {
    if offset >= DRIVER_TASK_RING_PAGE_BYTES {
        return 0;
    }
    // SAFETY: Runtime tests hold `test_guard` and the bounds check above keeps
    // reads inside the process-local fixed ring.
    unsafe { (*TEST_RING.0.get())[offset] }
}

#[cfg(all(not(target_os = "none"), not(test)))]
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

#[cfg(all(not(target_os = "none"), test))]
fn write_ring_byte(offset: usize, value: u8) {
    if offset >= DRIVER_TASK_RING_PAGE_BYTES {
        return;
    }
    // SAFETY: Runtime tests hold `test_guard` and the bounds check above keeps
    // writes inside the process-local fixed ring.
    unsafe {
        (*TEST_RING.0.get())[offset] = value;
    }
}

#[cfg(all(not(target_os = "none"), not(test)))]
fn write_ring_byte(_offset: usize, _value: u8) {}

fn write_ring_u32(offset: usize, value: u32) {
    write_ring_byte(offset, (value & 0xff) as u8);
    write_ring_byte(offset + 1, ((value >> 8) & 0xff) as u8);
    write_ring_byte(offset + 2, ((value >> 16) & 0xff) as u8);
    write_ring_byte(offset + 3, ((value >> 24) & 0xff) as u8);
}

#[cfg(any(target_os = "none", test))]
fn read_ring_u32(offset: usize) -> u32 {
    let b0 = u32::from(read_ring_byte(offset));
    let b1 = u32::from(read_ring_byte(offset + 1));
    let b2 = u32::from(read_ring_byte(offset + 2));
    let b3 = u32::from(read_ring_byte(offset + 3));
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

#[cfg(all(not(target_os = "none"), not(test)))]
fn read_ring_u32(_offset: usize) -> u32 {
    0
}

fn read_ring_u16(offset: usize) -> u16 {
    u16::from(read_ring_byte(offset)) | (u16::from(read_ring_byte(offset + 1)) << 8)
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
fn runtime_spin(iterations: usize) {
    for _ in 0..iterations {
        core::hint::spin_loop();
    }
}

#[cfg(not(target_os = "none"))]
fn runtime_spin(_iterations: usize) {}

fn dma_store_barrier() {
    core::sync::atomic::compiler_fence(Ordering::Release);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: A data-memory barrier is required before MMIO doorbells publish
    // runtime-written DMA descriptors to devices.
    unsafe {
        core::arch::asm!("dmb oshst", options(nostack, preserves_flags));
    }
}

fn dma_load_barrier() {
    core::sync::atomic::compiler_fence(Ordering::Acquire);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: A data-memory barrier is required before the runtime reads
    // device-written DMA payloads after descriptor completion.
    unsafe {
        core::arch::asm!("dmb oshld", options(nostack, preserves_flags));
    }
}

#[cfg(target_os = "none")]
fn read_dma_byte(addr: usize) -> u8 {
    // SAFETY: Callers construct `addr` from descriptor-validated runtime DMA
    // ranges mapped into the child VSpace.
    unsafe { core::ptr::read_volatile(addr as *const u8) }
}

#[cfg(not(target_os = "none"))]
fn read_dma_byte(_addr: usize) -> u8 {
    0
}

#[cfg(target_os = "none")]
fn write_dma_byte(addr: usize, value: u8) {
    // SAFETY: Callers construct `addr` from descriptor-validated runtime DMA
    // ranges mapped into the child VSpace.
    unsafe {
        core::ptr::write_volatile(addr as *mut u8, value);
    }
}

#[cfg(not(target_os = "none"))]
fn write_dma_byte(_addr: usize, _value: u8) {}

#[cfg(target_os = "none")]
fn genet_read32(offset: usize) -> u32 {
    // SAFETY: GENET offsets are bounded constants within the mapped GENET MMIO
    // range declared by the runtime descriptor.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u32) }
}

#[cfg(not(target_os = "none"))]
fn genet_read32(_offset: usize) -> u32 {
    0
}

#[cfg(target_os = "none")]
fn genet_write32(offset: usize, value: u32) {
    // SAFETY: GENET offsets are bounded constants within the mapped GENET MMIO
    // range declared by the runtime descriptor.
    unsafe {
        core::ptr::write_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *mut u32, value);
    }
}

#[cfg(not(target_os = "none"))]
fn genet_write32(_offset: usize, _value: u32) {}

#[cfg(target_os = "none")]
fn usb_read8(offset: usize) -> u8 {
    // SAFETY: The xHCI capability header is mapped at the runtime USB MMIO
    // base by the generated descriptor.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u8) }
}

#[cfg(not(target_os = "none"))]
fn usb_read8(_offset: usize) -> u8 {
    0
}

#[cfg(target_os = "none")]
fn usb_read32(offset: usize) -> u32 {
    // SAFETY: USB offsets are bounded by the xHCI capability/operational
    // layout inside the descriptor-validated USB MMIO range.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u32) }
}

#[cfg(not(target_os = "none"))]
fn usb_read32(_offset: usize) -> u32 {
    0
}

#[cfg(target_os = "none")]
fn usb_write32(offset: usize, value: u32) {
    // SAFETY: USB offsets are bounded by the xHCI capability/operational
    // layout inside the descriptor-validated USB MMIO range.
    unsafe {
        core::ptr::write_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *mut u32, value);
    }
}

#[cfg(not(target_os = "none"))]
fn usb_write32(_offset: usize, _value: u32) {}

fn usb_write64(offset: usize, value: u64) {
    usb_write32(offset, value as u32);
    usb_write32(offset + 4, (value >> 32) as u32);
}

fn zero_dma_range(vaddr: usize, len: usize) {
    for index in 0..len {
        write_dma_byte(vaddr + index, 0);
    }
}

fn write_dma_u32(addr: usize, value: u32) {
    for index in 0..4 {
        write_dma_byte(addr + index, ((value >> (index * 8)) & 0xff) as u8);
    }
}

fn write_dma_u64(addr: usize, value: u64) {
    for index in 0..8 {
        write_dma_byte(addr + index, ((value >> (index * 8)) & 0xff) as u8);
    }
}

fn read_dma_u32(addr: usize) -> u32 {
    let mut value = 0u32;
    for index in 0..4 {
        value |= u32::from(read_dma_byte(addr + index)) << (index * 8);
    }
    value
}

fn read_dma_u64(addr: usize) -> u64 {
    let mut value = 0u64;
    for index in 0..8 {
        value |= u64::from(read_dma_byte(addr + index)) << (index * 8);
    }
    value
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XhciTrb {
    parameter: u64,
    status: u32,
    control: u32,
}

fn xhci_trb_addr(base: usize, index: usize) -> usize {
    base + index.saturating_mul(XHCI_TRB_BYTES)
}

fn write_xhci_trb(base: usize, index: usize, trb: XhciTrb) {
    let addr = xhci_trb_addr(base, index);
    write_dma_u64(addr, trb.parameter);
    write_dma_u32(addr + 8, trb.status);
    dma_store_barrier();
    write_dma_u32(addr + 12, trb.control);
}

fn read_xhci_trb(base: usize, index: usize) -> XhciTrb {
    let addr = xhci_trb_addr(base, index);
    XhciTrb {
        parameter: read_dma_u64(addr),
        status: read_dma_u32(addr + 8),
        control: read_dma_u32(addr + 12),
    }
}

const fn xhci_trb_type(control: u32) -> u32 {
    (control >> 10) & 0x3f
}

const fn xhci_completion_code(status: u32) -> u32 {
    (status >> 24) & 0xff
}

const fn xhci_slot_id(control: u32) -> u8 {
    ((control >> 24) & 0xff) as u8
}

const fn xhci_endpoint_id(control: u32) -> u8 {
    ((control >> 16) & 0x1f) as u8
}

fn xhci_cycle_bit(cycle: bool) -> u32 {
    if cycle {
        XHCI_TRB_CYCLE
    } else {
        0
    }
}

fn xhci_dma_bus_addr(
    descriptor: DriverRuntimeInitDescriptor,
    dma_range_paddr: u64,
    offset: usize,
) -> u64 {
    runtime_bus_addr(descriptor, dma_range_paddr.saturating_add(offset as u64))
}

fn xhci_ring_doorbell(state: &UsbRuntimeState, slot: u8, endpoint_id: u8) {
    usb_write32(
        state.db_offset as usize + usize::from(slot).saturating_mul(4),
        u32::from(endpoint_id),
    );
    let _ = usb_read32(state.cap_length as usize + XHCI_USBSTS);
}

fn xhci_advance_ring(enqueue: &mut u16, cycle: &mut bool, trbs: usize) {
    *enqueue = enqueue.wrapping_add(1);
    if usize::from(*enqueue) >= trbs - 1 {
        *enqueue = 0;
        *cycle = !*cycle;
    }
}

fn xhci_ack_event_dequeue(state: &UsbRuntimeState, descriptor: DriverRuntimeInitDescriptor) {
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return;
    };
    let event = xhci_dma_bus_addr(
        descriptor,
        dma_range.paddr,
        XHCI_DMA_EVENT_RING_OFFSET
            + usize::from(state.event_dequeue).saturating_mul(XHCI_TRB_BYTES),
    );
    usb_write64(
        state.rt_offset as usize + 0x20 + XHCI_ERDP,
        event | (1 << 3),
    );
}

fn xhci_next_event(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
) -> Option<XhciTrb> {
    let dma_range = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    )?;
    let base = dma_range.vaddr as usize + XHCI_DMA_EVENT_RING_OFFSET;
    dma_load_barrier();
    let trb = read_xhci_trb(base, usize::from(state.event_dequeue));
    if (trb.control & XHCI_TRB_CYCLE != 0) != state.event_cycle {
        return None;
    }
    state.event_dequeue = state.event_dequeue.wrapping_add(1);
    if usize::from(state.event_dequeue) >= XHCI_EVENT_RING_TRBS {
        state.event_dequeue = 0;
        state.event_cycle = !state.event_cycle;
    }
    Some(trb)
}

fn xhci_enqueue_command(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    mut trb: XhciTrb,
) -> Option<XhciTrb> {
    let dma_range = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    )?;
    let base = dma_range.vaddr as usize + XHCI_DMA_COMMAND_RING_OFFSET;
    let index = usize::from(state.cmd_enqueue);
    if index >= XHCI_COMMAND_RING_TRBS - 1 {
        return None;
    }
    trb.control |= xhci_cycle_bit(state.cmd_cycle);
    write_xhci_trb(base, index, trb);
    state.cmd_enqueue = state.cmd_enqueue.wrapping_add(1);
    if usize::from(state.cmd_enqueue) >= XHCI_COMMAND_RING_TRBS - 1 {
        state.cmd_enqueue = 0;
        state.cmd_cycle = !state.cmd_cycle;
    }
    xhci_ring_doorbell(state, 0, 0);
    xhci_wait_command_completion(state, descriptor)
}

fn xhci_wait_command_completion(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
) -> Option<XhciTrb> {
    for _ in 0..USB_XHCI_SPINS {
        while let Some(event) = xhci_next_event(state, descriptor) {
            xhci_ack_event_dequeue(state, descriptor);
            if xhci_trb_type(event.control) == XHCI_TRB_TYPE_COMMAND_COMPLETION {
                let code = xhci_completion_code(event.status);
                if code == XHCI_COMPLETION_SUCCESS {
                    return Some(event);
                }
                return None;
            }
        }
        core::hint::spin_loop();
    }
    None
}

fn xhci_wait_transfer_completion(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    slot: u8,
    endpoint_id: u8,
) -> bool {
    for _ in 0..USB_XHCI_SPINS {
        while let Some(event) = xhci_next_event(state, descriptor) {
            xhci_ack_event_dequeue(state, descriptor);
            if xhci_trb_type(event.control) == XHCI_TRB_TYPE_TRANSFER_EVENT
                && xhci_slot_id(event.control) == slot
                && xhci_endpoint_id(event.control) == endpoint_id
            {
                let code = xhci_completion_code(event.status);
                return code == XHCI_COMPLETION_SUCCESS || code == XHCI_COMPLETION_SHORT_PACKET;
            }
        }
        core::hint::spin_loop();
    }
    false
}

fn xhci_context_addr(dma_base: usize, offset: usize, context_bytes: usize, index: usize) -> usize {
    dma_base + offset + context_bytes.saturating_mul(index)
}

fn xhci_write_context_u32(
    dma_base: usize,
    offset: usize,
    context_bytes: usize,
    index: usize,
    word: usize,
    value: u32,
) {
    write_dma_u32(
        xhci_context_addr(dma_base, offset, context_bytes, index) + word.saturating_mul(4),
        value,
    );
}

fn xhci_prepare_contexts(
    state: &UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    dma_range: DriverRuntimeResourceRangeDescriptor,
    port: u8,
    speed: u32,
    endpoint_id: u8,
) {
    let dma_base = dma_range.vaddr as usize;
    zero_dma_range(dma_base + XHCI_DMA_INPUT_CONTEXT_OFFSET, 0x2000);
    let add_flags = if endpoint_id == 0 {
        (1 << 0) | (1 << 1)
    } else {
        (1 << 0) | (1u32 << endpoint_id)
    };
    write_dma_u32(dma_base + XHCI_DMA_INPUT_CONTEXT_OFFSET + 4, add_flags);
    let context_entries = if endpoint_id == 0 {
        1
    } else {
        u32::from(endpoint_id)
    };
    xhci_write_context_u32(
        dma_base,
        XHCI_DMA_INPUT_CONTEXT_OFFSET,
        state.context_bytes,
        1,
        0,
        (speed << XHCI_SLOT_SPEED_SHIFT) | (context_entries << XHCI_CONTEXT_ENTRIES_SHIFT),
    );
    xhci_write_context_u32(
        dma_base,
        XHCI_DMA_INPUT_CONTEXT_OFFSET,
        state.context_bytes,
        1,
        1,
        u32::from(port) << XHCI_SLOT_ROOT_HUB_PORT_SHIFT,
    );
    let ep0_ring = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_EP0_RING_OFFSET);
    xhci_write_context_u32(
        dma_base,
        XHCI_DMA_INPUT_CONTEXT_OFFSET,
        state.context_bytes,
        2,
        1,
        (XHCI_ENDPOINT_TYPE_CONTROL << XHCI_EP_TYPE_SHIFT)
            | (XHCI_DEFAULT_CONTROL_PACKET << XHCI_EP_MAX_PACKET_SHIFT),
    );
    xhci_write_context_u32(
        dma_base,
        XHCI_DMA_INPUT_CONTEXT_OFFSET,
        state.context_bytes,
        2,
        2,
        ep0_ring as u32 | 1,
    );
    xhci_write_context_u32(
        dma_base,
        XHCI_DMA_INPUT_CONTEXT_OFFSET,
        state.context_bytes,
        2,
        3,
        (ep0_ring >> 32) as u32,
    );
    xhci_write_context_u32(
        dma_base,
        XHCI_DMA_INPUT_CONTEXT_OFFSET,
        state.context_bytes,
        2,
        4,
        8,
    );
    if endpoint_id != 0 {
        let kbd_ring = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_KBD_RING_OFFSET);
        xhci_write_context_u32(
            dma_base,
            XHCI_DMA_INPUT_CONTEXT_OFFSET,
            state.context_bytes,
            usize::from(endpoint_id) + 1,
            0,
            u32::from(state.keyboard_ep_interval) << 16,
        );
        xhci_write_context_u32(
            dma_base,
            XHCI_DMA_INPUT_CONTEXT_OFFSET,
            state.context_bytes,
            usize::from(endpoint_id) + 1,
            1,
            (XHCI_ENDPOINT_TYPE_INTERRUPT_IN << XHCI_EP_TYPE_SHIFT)
                | (u32::from(state.keyboard_ep_max_packet) << XHCI_EP_MAX_PACKET_SHIFT),
        );
        xhci_write_context_u32(
            dma_base,
            XHCI_DMA_INPUT_CONTEXT_OFFSET,
            state.context_bytes,
            usize::from(endpoint_id) + 1,
            2,
            kbd_ring as u32 | 1,
        );
        xhci_write_context_u32(
            dma_base,
            XHCI_DMA_INPUT_CONTEXT_OFFSET,
            state.context_bytes,
            usize::from(endpoint_id) + 1,
            3,
            (kbd_ring >> 32) as u32,
        );
        xhci_write_context_u32(
            dma_base,
            XHCI_DMA_INPUT_CONTEXT_OFFSET,
            state.context_bytes,
            usize::from(endpoint_id) + 1,
            4,
            u32::from(state.keyboard_ep_max_packet),
        );
    }
}

fn xhci_prepare_dma_structures(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    dma_range: DriverRuntimeResourceRangeDescriptor,
) {
    let dma_base = dma_range.vaddr as usize;
    zero_dma_range(
        dma_base,
        XHCI_DMA_REPORT_BUFFER_OFFSET + DRIVER_TASK_RING_PAGE_BYTES,
    );
    let cmd_ring_bus = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_COMMAND_RING_OFFSET);
    let event_ring_bus = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_EVENT_RING_OFFSET);
    let event_base = dma_base + XHCI_DMA_EVENT_RING_OFFSET;
    let cmd_base = dma_base + XHCI_DMA_COMMAND_RING_OFFSET;
    write_xhci_trb(
        cmd_base,
        XHCI_COMMAND_RING_TRBS - 1,
        XhciTrb {
            parameter: cmd_ring_bus,
            status: 0,
            control: (XHCI_TRB_TYPE_LINK << 10) | XHCI_TRB_ENT,
        },
    );
    write_xhci_trb(
        dma_base + XHCI_DMA_EP0_RING_OFFSET,
        XHCI_COMMAND_RING_TRBS - 1,
        XhciTrb {
            parameter: xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_EP0_RING_OFFSET),
            status: 0,
            control: (XHCI_TRB_TYPE_LINK << 10) | XHCI_TRB_ENT,
        },
    );
    write_xhci_trb(
        dma_base + XHCI_DMA_KBD_RING_OFFSET,
        XHCI_COMMAND_RING_TRBS - 1,
        XhciTrb {
            parameter: xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_KBD_RING_OFFSET),
            status: 0,
            control: (XHCI_TRB_TYPE_LINK << 10) | XHCI_TRB_ENT,
        },
    );
    write_dma_u64(dma_base + XHCI_DMA_ERST_OFFSET, event_ring_bus);
    write_dma_u32(
        dma_base + XHCI_DMA_ERST_OFFSET + 8,
        XHCI_EVENT_RING_TRBS as u32,
    );
    if state.scratchpad_count != 0 {
        let array_bus = xhci_dma_bus_addr(
            descriptor,
            dma_range.paddr,
            XHCI_DMA_SCRATCHPAD_ARRAY_OFFSET,
        );
        write_dma_u64(dma_base, array_bus);
        let count = state.scratchpad_count.min(16);
        for index in 0..count {
            let scratch_bus = xhci_dma_bus_addr(
                descriptor,
                dma_range.paddr,
                XHCI_DMA_SCRATCHPAD_OFFSET + index * DRIVER_TASK_RING_PAGE_BYTES,
            );
            write_dma_u64(
                dma_base + XHCI_DMA_SCRATCHPAD_ARRAY_OFFSET + index.saturating_mul(8),
                scratch_bus,
            );
        }
    }
    state.cmd_enqueue = 0;
    state.cmd_cycle = true;
    state.event_dequeue = 0;
    state.event_cycle = true;
    state.ep0_enqueue = 0;
    state.ep0_cycle = true;
    state.kbd_enqueue = 0;
    state.kbd_cycle = true;
    let _ = event_base;
}

fn xhci_reset_root_port(state: &UsbRuntimeState, port: u8) -> Option<u32> {
    if port == 0 || port > state.max_ports {
        return None;
    }
    let op_base = state.cap_length as usize;
    let portsc = op_base + XHCI_PORTSC_BASE + (usize::from(port) - 1) * XHCI_PORTSC_STRIDE;
    let status = usb_read32(portsc);
    if status & XHCI_PORTSC_CCS == 0 {
        return None;
    }
    usb_write32(
        portsc,
        (status | XHCI_PORTSC_PP | XHCI_PORTSC_PR) & !XHCI_PORTSC_PED,
    );
    for _ in 0..USB_XHCI_SPINS {
        let next = usb_read32(portsc);
        if next & XHCI_PORTSC_PR == 0 && next & XHCI_PORTSC_PRC != 0 {
            usb_write32(
                portsc,
                next | XHCI_PORTSC_CSC | XHCI_PORTSC_PEC | XHCI_PORTSC_PRC,
            );
            if next & XHCI_PORTSC_PED != 0 {
                return Some((next >> XHCI_PORTSC_SPEED_SHIFT) & XHCI_PORTSC_SPEED_MASK);
            }
        }
        core::hint::spin_loop();
    }
    None
}

fn xhci_enable_slot(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
) -> Option<u8> {
    let event = xhci_enqueue_command(
        state,
        descriptor,
        XhciTrb {
            parameter: 0,
            status: 0,
            control: XHCI_TRB_TYPE_ENABLE_SLOT << 10,
        },
    )?;
    let slot = xhci_slot_id(event.control);
    (slot != 0).then_some(slot)
}

fn xhci_address_device(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    dma_range: DriverRuntimeResourceRangeDescriptor,
    slot: u8,
    port: u8,
    speed: u32,
) -> bool {
    zero_dma_range(
        dma_range.vaddr as usize + XHCI_DMA_DEVICE_CONTEXT_OFFSET,
        0x2000,
    );
    xhci_prepare_contexts(state, descriptor, dma_range, port, speed, 0);
    let input_ctx = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_INPUT_CONTEXT_OFFSET);
    let device_ctx = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_DEVICE_CONTEXT_OFFSET);
    write_dma_u64(
        dma_range.vaddr as usize + usize::from(slot).saturating_mul(8),
        device_ctx,
    );
    xhci_enqueue_command(
        state,
        descriptor,
        XhciTrb {
            parameter: input_ctx,
            status: 0,
            control: (XHCI_TRB_TYPE_ADDRESS_DEVICE << 10) | (u32::from(slot) << 24),
        },
    )
    .is_some()
}

fn xhci_control_transfer(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    setup: [u8; 8],
    data_offset: usize,
    data_len: usize,
    data_in: bool,
) -> bool {
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return false;
    };
    let base = dma_range.vaddr as usize + XHCI_DMA_EP0_RING_OFFSET;
    let data_bus = xhci_dma_bus_addr(descriptor, dma_range.paddr, data_offset);
    let setup_value = u64::from(setup[0])
        | (u64::from(setup[1]) << 8)
        | (u64::from(setup[2]) << 16)
        | (u64::from(setup[3]) << 24)
        | (u64::from(setup[4]) << 32)
        | (u64::from(setup[5]) << 40)
        | (u64::from(setup[6]) << 48)
        | (u64::from(setup[7]) << 56);
    let setup_index = usize::from(state.ep0_enqueue);
    let setup_cycle = state.ep0_cycle;
    let setup_transfer_type = if data_len == 0 {
        0
    } else if data_in {
        XHCI_TRB_TRANSFER_TYPE_IN
    } else {
        XHCI_TRB_TRANSFER_TYPE_OUT
    };
    write_xhci_trb(
        base,
        setup_index,
        XhciTrb {
            parameter: setup_value,
            status: 8,
            control: (XHCI_TRB_TYPE_SETUP_STAGE << 10)
                | XHCI_TRB_IDT
                | setup_transfer_type
                | xhci_cycle_bit(setup_cycle),
        },
    );
    xhci_advance_ring(
        &mut state.ep0_enqueue,
        &mut state.ep0_cycle,
        XHCI_COMMAND_RING_TRBS,
    );
    if data_len != 0 {
        write_xhci_trb(
            base,
            usize::from(state.ep0_enqueue),
            XhciTrb {
                parameter: data_bus,
                status: data_len as u32,
                control: (XHCI_TRB_TYPE_DATA_STAGE << 10)
                    | (if data_in { XHCI_TRB_DIR_IN } else { 0 })
                    | xhci_cycle_bit(state.ep0_cycle),
            },
        );
        xhci_advance_ring(
            &mut state.ep0_enqueue,
            &mut state.ep0_cycle,
            XHCI_COMMAND_RING_TRBS,
        );
    }
    write_xhci_trb(
        base,
        usize::from(state.ep0_enqueue),
        XhciTrb {
            parameter: 0,
            status: 0,
            control: (XHCI_TRB_TYPE_STATUS_STAGE << 10)
                | (if data_in { 0 } else { XHCI_TRB_DIR_IN })
                | XHCI_TRB_IOC
                | xhci_cycle_bit(state.ep0_cycle),
        },
    );
    xhci_advance_ring(
        &mut state.ep0_enqueue,
        &mut state.ep0_cycle,
        XHCI_COMMAND_RING_TRBS,
    );
    xhci_ring_doorbell(state, state.keyboard_slot, 1);
    xhci_wait_transfer_completion(state, descriptor, state.keyboard_slot, 1)
}

fn usb_get_descriptor(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    desc_type: u8,
    index: u8,
    offset: usize,
    len: usize,
) -> bool {
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return false;
    };
    zero_dma_range(dma_range.vaddr as usize + offset, len);
    let setup = [
        0x80,
        XHCI_SETUP_GET_DESCRIPTOR,
        index,
        desc_type,
        0,
        0,
        (len & 0xff) as u8,
        (len >> 8) as u8,
    ];
    xhci_control_transfer(state, descriptor, setup, offset, len, true)
}

fn usb_set_configuration(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    configuration: u8,
) -> bool {
    let setup = [
        0x00,
        XHCI_SETUP_SET_CONFIGURATION,
        configuration,
        0,
        0,
        0,
        0,
        0,
    ];
    xhci_control_transfer(
        state,
        descriptor,
        setup,
        XHCI_DMA_CONTROL_BUFFER_OFFSET,
        0,
        false,
    )
}

fn usb_set_hid_boot_protocol(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
) -> bool {
    let setup = [
        0x21,
        XHCI_SETUP_SET_PROTOCOL,
        0,
        0,
        state.keyboard_interface,
        0,
        0,
        0,
    ];
    xhci_control_transfer(
        state,
        descriptor,
        setup,
        XHCI_DMA_CONTROL_BUFFER_OFFSET,
        0,
        false,
    )
}

fn usb_set_idle(state: &mut UsbRuntimeState, descriptor: DriverRuntimeInitDescriptor) -> bool {
    let setup = [
        0x21,
        XHCI_SETUP_SET_IDLE,
        0,
        0,
        state.keyboard_interface,
        0,
        0,
        0,
    ];
    xhci_control_transfer(
        state,
        descriptor,
        setup,
        XHCI_DMA_CONTROL_BUFFER_OFFSET,
        0,
        false,
    )
}

fn usb_parse_keyboard_endpoint(
    state: &mut UsbRuntimeState,
    config_vaddr: usize,
    len: usize,
) -> bool {
    if len < 9 {
        return false;
    }
    let configuration_value = read_dma_byte(config_vaddr + 5);
    let mut offset = 0usize;
    let mut current_keyboard_iface = None;
    while offset + 2 <= len {
        let desc_len = read_dma_byte(config_vaddr + offset) as usize;
        let desc_type = read_dma_byte(config_vaddr + offset + 1);
        if desc_len < 2 || offset + desc_len > len {
            break;
        }
        if desc_type == USB_DESCRIPTOR_INTERFACE && desc_len >= 9 {
            let interface = read_dma_byte(config_vaddr + offset + 2);
            let class = read_dma_byte(config_vaddr + offset + 5);
            let subclass = read_dma_byte(config_vaddr + offset + 6);
            let protocol = read_dma_byte(config_vaddr + offset + 7);
            current_keyboard_iface = (class == USB_CLASS_HID
                && subclass == USB_SUBCLASS_BOOT
                && protocol == USB_PROTOCOL_KEYBOARD)
                .then_some(interface);
        } else if desc_type == USB_DESCRIPTOR_ENDPOINT && desc_len >= 7 {
            if let Some(interface) = current_keyboard_iface {
                let address = read_dma_byte(config_vaddr + offset + 2);
                let attrs = read_dma_byte(config_vaddr + offset + 3);
                if address & USB_ENDPOINT_DIR_IN != 0
                    && (attrs & 0x3) == USB_ENDPOINT_ATTR_INTERRUPT
                {
                    let endpoint_num = address & 0x0f;
                    let max_packet = u16::from(read_dma_byte(config_vaddr + offset + 4))
                        | (u16::from(read_dma_byte(config_vaddr + offset + 5)) << 8);
                    state.keyboard_endpoint_address = address;
                    state.keyboard_endpoint_id = endpoint_num.saturating_mul(2).saturating_add(1);
                    state.keyboard_interface = interface;
                    state.keyboard_ep_interval = read_dma_byte(config_vaddr + offset + 6);
                    state.keyboard_ep_max_packet = max_packet.max(XHCI_BOOT_REPORT_BYTES as u16);
                    return configuration_value != 0;
                }
            }
        }
        offset += desc_len;
    }
    false
}

fn xhci_configure_keyboard_endpoint(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    dma_range: DriverRuntimeResourceRangeDescriptor,
    speed: u32,
) -> bool {
    xhci_prepare_contexts(
        state,
        descriptor,
        dma_range,
        state.keyboard_port,
        speed,
        state.keyboard_endpoint_id,
    );
    let input_ctx = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_INPUT_CONTEXT_OFFSET);
    xhci_enqueue_command(
        state,
        descriptor,
        XhciTrb {
            parameter: input_ctx,
            status: 0,
            control: (XHCI_TRB_TYPE_CONFIGURE_ENDPOINT << 10)
                | (u32::from(state.keyboard_slot) << 24),
        },
    )
    .is_some()
}

fn usb_keyboard_enumerate(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
    dma_range: DriverRuntimeResourceRangeDescriptor,
) -> bool {
    let mut selected_port = 0u8;
    let mut selected_speed = 0u32;
    for port in 1..=state.max_ports {
        if let Some(speed) = xhci_reset_root_port(state, port) {
            selected_port = port;
            selected_speed = speed;
            break;
        }
    }
    if selected_port == 0 {
        return false;
    }
    let Some(slot) = xhci_enable_slot(state, descriptor) else {
        return false;
    };
    state.keyboard_slot = slot;
    state.keyboard_port = selected_port;
    if !xhci_address_device(
        state,
        descriptor,
        dma_range,
        slot,
        selected_port,
        selected_speed,
    ) {
        return false;
    }
    if !usb_get_descriptor(
        state,
        descriptor,
        USB_DESCRIPTOR_DEVICE,
        0,
        XHCI_DMA_CONTROL_BUFFER_OFFSET,
        18,
    ) {
        return false;
    }
    if !usb_get_descriptor(
        state,
        descriptor,
        USB_DESCRIPTOR_CONFIGURATION,
        0,
        XHCI_DMA_CONFIG_BUFFER_OFFSET,
        9,
    ) {
        return false;
    }
    let config_vaddr = dma_range.vaddr as usize + XHCI_DMA_CONFIG_BUFFER_OFFSET;
    let total_len = usize::from(read_dma_byte(config_vaddr + 2))
        | (usize::from(read_dma_byte(config_vaddr + 3)) << 8);
    let config_len = total_len.min(512).max(9);
    if !usb_get_descriptor(
        state,
        descriptor,
        USB_DESCRIPTOR_CONFIGURATION,
        0,
        XHCI_DMA_CONFIG_BUFFER_OFFSET,
        config_len,
    ) {
        return false;
    }
    if !usb_parse_keyboard_endpoint(state, config_vaddr, config_len) {
        return false;
    }
    let config_value = read_dma_byte(config_vaddr + 5);
    usb_set_configuration(state, descriptor, config_value)
        && usb_set_hid_boot_protocol(state, descriptor)
        && usb_set_idle(state, descriptor)
        && xhci_configure_keyboard_endpoint(state, descriptor, dma_range, selected_speed)
}

fn xhci_queue_keyboard_interrupt_in(
    state: &mut UsbRuntimeState,
    descriptor: DriverRuntimeInitDescriptor,
) -> bool {
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return false;
    };
    let base = dma_range.vaddr as usize + XHCI_DMA_KBD_RING_OFFSET;
    let report_bus = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_REPORT_BUFFER_OFFSET);
    zero_dma_range(
        dma_range.vaddr as usize + XHCI_DMA_REPORT_BUFFER_OFFSET,
        XHCI_BOOT_REPORT_BYTES,
    );
    write_xhci_trb(
        base,
        usize::from(state.kbd_enqueue),
        XhciTrb {
            parameter: report_bus,
            status: XHCI_BOOT_REPORT_BYTES as u32,
            control: (XHCI_TRB_TYPE_NORMAL << 10) | XHCI_TRB_IOC | xhci_cycle_bit(state.kbd_cycle),
        },
    );
    xhci_advance_ring(
        &mut state.kbd_enqueue,
        &mut state.kbd_cycle,
        XHCI_COMMAND_RING_TRBS,
    );
    state.keyboard_report_queued = true;
    xhci_ring_doorbell(state, state.keyboard_slot, state.keyboard_endpoint_id);
    true
}

fn usb_wait_status(op_base: usize, mask: u32, expected: u32) -> bool {
    for _ in 0..PCIE_POLL_SPINS {
        if usb_read32(op_base + XHCI_USBSTS) & mask == expected {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn usb_wait_command_clear(op_base: usize, mask: u32) -> bool {
    for _ in 0..PCIE_POLL_SPINS {
        if usb_read32(op_base + XHCI_USBCMD) & mask == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn usb_runtime_init_hw(
    descriptor: DriverRuntimeInitDescriptor,
    state: &mut UsbRuntimeState,
) -> bool {
    let Some(dma_range) = runtime_resource_range(
        descriptor,
        DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    ) else {
        return false;
    };
    if dma_range.page_count < USB_REQUIRED_DMA_PAGES {
        return false;
    }
    let cap_length = usb_read8(XHCI_CAPLENGTH);
    let hcs1 = usb_read32(XHCI_HCSPARAMS1);
    let hcs2 = usb_read32(XHCI_HCSPARAMS2);
    let hcc = usb_read32(XHCI_HCCPARAMS1);
    let db_offset = usb_read32(XHCI_DBOFF) & !0x3;
    let rt_offset = usb_read32(XHCI_RTSOFF) & !0x1f;
    let max_slots = (hcs1 & 0xff) as u8;
    let max_ports = ((hcs1 >> 24) & 0xff) as u8;
    if cap_length == 0 || max_slots == 0 || max_ports == 0 || db_offset == 0 || rt_offset == 0 {
        #[cfg(not(target_os = "none"))]
        {
            state.cap_length = 0x40;
            state.max_slots = 32;
            state.max_ports = 4;
            state.db_offset = 0x1000;
            state.rt_offset = 0x2000;
            state.context_bytes = 32;
            return true;
        }
        #[cfg(target_os = "none")]
        return false;
    }
    let op_base = cap_length as usize;
    state.context_bytes = if hcc & (1 << 2) != 0 { 64 } else { 32 };
    state.scratchpad_count = (((hcs2 >> 27) & 0x1f) | ((hcs2 >> 16) & 0x3e0)) as usize;
    state.cap_length = cap_length;
    state.max_slots = max_slots;
    state.max_ports = max_ports;
    state.db_offset = db_offset;
    state.rt_offset = rt_offset;
    usb_write32(
        op_base + XHCI_USBCMD,
        usb_read32(op_base + XHCI_USBCMD) & !XHCI_USBCMD_RUN,
    );
    if !usb_wait_status(op_base, XHCI_USBSTS_HCH, XHCI_USBSTS_HCH) {
        return false;
    }
    usb_write32(op_base + XHCI_USBCMD, XHCI_USBCMD_HCRST);
    if !usb_wait_command_clear(op_base, XHCI_USBCMD_HCRST) {
        return false;
    }
    if !usb_wait_status(op_base, XHCI_USBSTS_CNR, 0) {
        return false;
    }
    xhci_prepare_dma_structures(state, descriptor, dma_range);
    let dcbaa = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_DCBBA_OFFSET);
    let cmd_ring = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_COMMAND_RING_OFFSET);
    let event_ring = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_EVENT_RING_OFFSET);
    let erst = xhci_dma_bus_addr(descriptor, dma_range.paddr, XHCI_DMA_ERST_OFFSET);
    dma_store_barrier();
    usb_write64(op_base + XHCI_DCBAAP, dcbaa);
    usb_write64(op_base + XHCI_CRCR, cmd_ring | 1);
    usb_write32(op_base + XHCI_DNCTRL, 0);
    usb_write32(op_base + XHCI_CONFIG, max_slots as u32);
    let int_base = rt_offset as usize + 0x20;
    usb_write32(int_base + XHCI_IMAN, 1);
    usb_write32(int_base + XHCI_ERSTSZ, 1);
    usb_write64(int_base + XHCI_ERSTBA, erst);
    usb_write64(int_base + XHCI_ERDP, event_ring | (1 << 3));
    dma_store_barrier();
    usb_write32(op_base + XHCI_USBCMD, XHCI_USBCMD_RUN);
    let _ = usb_wait_status(op_base, XHCI_USBSTS_HCH, 0);
    let _ = usb_keyboard_enumerate(state, descriptor, dma_range);
    true
}

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

fn pcie_runtime_init_hw(state: &mut PcieRuntimeState) -> bool {
    let mut misc = pcie_read32(PCIE_MISC_MISC_CTRL);
    misc |= PCIE_MISC_MISC_CTRL_SCB_ACCESS_EN_MASK | PCIE_MISC_MISC_CTRL_CFG_READ_UR_MODE_MASK;
    misc &= !PCIE_MISC_MISC_CTRL_MAX_BURST_SIZE_MASK;
    misc &= !PCIE_MISC_MISC_CTRL_SCB0_SIZE_MASK;
    pcie_write32(PCIE_MISC_MISC_CTRL, misc);
    pcie_write32(PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LO, 0xc000_0000);
    pcie_write32(PCIE_MISC_CPU_2_PCIE_MEM_WIN0_HI, 0);
    pcie_write32(PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_LIMIT, 0xfff0_c000);
    pcie_write32(PCIE_MISC_CPU_2_PCIE_MEM_WIN0_BASE_HI, 0);
    pcie_write32(PCIE_MISC_CPU_2_PCIE_MEM_WIN0_LIMIT_HI, 0);
    pcie_write32(
        PCIE_MISC_RC_BAR1_CONFIG_LO,
        pcie_read32(PCIE_MISC_RC_BAR1_CONFIG_LO) & !PCIE_MISC_RC_BAR1_CONFIG_LO_SIZE_MASK,
    );
    pcie_write32(
        PCIE_MISC_RC_BAR2_CONFIG_LO,
        pcie_read32(PCIE_MISC_RC_BAR2_CONFIG_LO) & !PCIE_MISC_RC_BAR2_CONFIG_LO_SIZE_MASK,
    );
    pcie_write32(PCIE_MISC_RC_BAR2_CONFIG_HI, 0);
    pcie_write32(
        PCIE_MISC_RC_BAR3_CONFIG_LO,
        pcie_read32(PCIE_MISC_RC_BAR3_CONFIG_LO) & !PCIE_MISC_RC_BAR3_CONFIG_LO_SIZE_MASK,
    );
    pcie_write32(PCIE_INTR2_CPU_CLR, u32::MAX);
    pcie_write32(PCIE_INTR2_CPU_MASK_SET, u32::MAX);
    pcie_write32(PCIE_MSI_INTR2_CLR, u32::MAX);
    pcie_write32(PCIE_MSI_INTR2_MASK_SET, u32::MAX);
    pcie_write32(
        PCIE_MISC_HARD_PCIE_HARD_DEBUG,
        pcie_read32(PCIE_MISC_HARD_PCIE_HARD_DEBUG) & !PCIE_HARD_DEBUG_SERDES_IDDQ_MASK,
    );
    pcie_write32(
        PCIE_RGR1_SW_INIT_1,
        PCIE_RGR1_SW_INIT_1_INIT_MASK | PCIE_RGR1_SW_INIT_1_PERST_MASK,
    );
    runtime_spin(PCIE_POLL_SPINS / 10);
    pcie_write32(
        PCIE_RGR1_SW_INIT_1,
        pcie_read32(PCIE_RGR1_SW_INIT_1) & !PCIE_RGR1_SW_INIT_1_INIT_MASK,
    );
    runtime_spin(PCIE_POLL_SPINS / 10);
    pcie_write32(
        PCIE_RGR1_SW_INIT_1,
        pcie_read32(PCIE_RGR1_SW_INIT_1) & !PCIE_RGR1_SW_INIT_1_PERST_MASK,
    );
    runtime_spin(PCIE_POLL_SPINS);
    let status = pcie_read32(PCIE_MISC_PCIE_STATUS);
    state.last_status = status;
    state.link_ready = status
        & (PCIE_STATUS_PORT | PCIE_STATUS_DL_ACTIVE | PCIE_STATUS_PHY_LINK_UP)
        == (PCIE_STATUS_PORT | PCIE_STATUS_DL_ACTIVE | PCIE_STATUS_PHY_LINK_UP);
    pcie_cfg_select();
    state.cfg_vendor_device = pcie_cfg_read32(0);
    state.cfg_class_revision = pcie_cfg_read32(PCIE_CFG_CLASS_REV);
    if pcie_cfg_read32(PCIE_CFG_BAR0) == 0 || pcie_cfg_read32(PCIE_CFG_BAR1) == 0 {
        pcie_cfg_write32(PCIE_CFG_BAR1, 0);
        pcie_cfg_write32(PCIE_CFG_BAR0, PCIE_VL805_ASSIGNED_BAR0);
    }
    let command = pcie_cfg_read32(PCIE_CFG_COMMAND)
        | PCIE_COMMAND_MEMORY_SPACE
        | PCIE_COMMAND_BUS_MASTER
        | PCIE_COMMAND_INTX_DISABLE;
    pcie_cfg_write32(PCIE_CFG_COMMAND, command);
    #[cfg(not(target_os = "none"))]
    {
        state.link_ready = true;
        state.cfg_vendor_device = PCIE_VL805_PCI_VENDOR_DEVICE;
        state.cfg_class_revision = PCIE_VL805_EXPECTED_CLASS_REV;
    }
    state.link_ready
        && state.cfg_vendor_device == PCIE_VL805_PCI_VENDOR_DEVICE
        && (state.cfg_class_revision & 0xffff_ff00) == PCIE_VL805_EXPECTED_CLASS_REV
}

fn pcie_cfg_select() {
    pcie_write32(PCIE_EXT_CFG_INDEX, PCIE_VL805_PCI_DEV_ADDR);
    runtime_spin(1024);
}

fn pcie_cfg_read32(offset: usize) -> u32 {
    pcie_cfg_select();
    pcie_read32(PCIE_EXT_CFG_DATA + offset)
}

fn pcie_cfg_write32(offset: usize, value: u32) {
    pcie_cfg_select();
    pcie_write32(PCIE_EXT_CFG_DATA + offset, value);
}

#[cfg(target_os = "none")]
fn sdio_read32(offset: usize) -> u32 {
    // SAFETY: The SDIO runtime maps the declared SDHCI MMIO page at
    // `DRIVER_TASK_DEVICE_MMIO_VADDR`; all offsets used by callers are bounded
    // register constants within that page.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u32) }
}

#[cfg(not(target_os = "none"))]
fn sdio_read32(_offset: usize) -> u32 {
    0
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

#[cfg(not(target_os = "none"))]
fn sdio_write32(_offset: usize, _value: u32) {}

#[cfg(target_os = "none")]
fn sdio_read16(offset: usize) -> u16 {
    // SAFETY: The SDIO runtime maps the declared SDHCI MMIO page at the fixed
    // runtime MMIO base.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u16) }
}

#[cfg(not(target_os = "none"))]
fn sdio_read16(_offset: usize) -> u16 {
    SDHCI_CLOCK_INT_STABLE
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

#[cfg(not(target_os = "none"))]
fn sdio_write16(_offset: usize, _value: u16) {}

#[cfg(target_os = "none")]
fn sdio_read8(offset: usize) -> u8 {
    // SAFETY: The SDIO runtime maps the declared SDHCI MMIO page at the fixed
    // runtime MMIO base.
    unsafe { core::ptr::read_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *const u8) }
}

#[cfg(not(target_os = "none"))]
fn sdio_read8(offset: usize) -> u8 {
    if offset == SDHCI_SOFTWARE_RESET {
        0
    } else if offset == SDHCI_POWER_CONTROL {
        SDHCI_POWER_330 | SDHCI_POWER_ON
    } else {
        0
    }
}

#[cfg(target_os = "none")]
fn sdio_write8(offset: usize, value: u8) {
    // SAFETY: The SDIO runtime maps the declared SDHCI MMIO page at the fixed
    // runtime MMIO base.
    unsafe {
        core::ptr::write_volatile((DRIVER_TASK_DEVICE_MMIO_VADDR + offset) as *mut u8, value);
    }
}

#[cfg(not(target_os = "none"))]
fn sdio_write8(_offset: usize, _value: u8) {}

fn sdio_runtime_init_hw() -> bool {
    sdio_write16(SDHCI_CLOCK_CONTROL, 0);
    sdio_write8(SDHCI_POWER_CONTROL, 0);
    sdio_write8(SDHCI_SOFTWARE_RESET, SDHCI_RESET_ALL);
    for _ in 0..SDHCI_INIT_SPINS {
        if sdio_read8(SDHCI_SOFTWARE_RESET) & SDHCI_RESET_ALL == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    sdio_write8(SDHCI_POWER_CONTROL, SDHCI_POWER_330 | SDHCI_POWER_ON);
    sdio_write8(SDHCI_TIMEOUT_CONTROL, 0x0e);
    sdio_write32(SDHCI_INT_ENABLE, SDHCI_INT_COMMAND_DATA_CLEAR_MASK);
    sdio_write32(SDHCI_SIGNAL_ENABLE, 0);
    sdio_write8(SDHCI_SOFTWARE_RESET, SDHCI_RESET_CMD | SDHCI_RESET_DATA);
    for _ in 0..SDHCI_INIT_SPINS {
        if sdio_read8(SDHCI_SOFTWARE_RESET) & (SDHCI_RESET_CMD | SDHCI_RESET_DATA) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
    sdio_write16(SDHCI_CLOCK_CONTROL, SDHCI_CLOCK_INT_EN);
    for _ in 0..SDHCI_INIT_SPINS {
        if sdio_read16(SDHCI_CLOCK_CONTROL) & SDHCI_CLOCK_INT_STABLE != 0 {
            break;
        }
        core::hint::spin_loop();
    }
    sdio_write16(
        SDHCI_CLOCK_CONTROL,
        SDHCI_CLOCK_INT_EN | SDHCI_CLOCK_CARD_EN,
    );
    sdio_write8(SDHCI_HOST_CONTROL, sdio_read8(SDHCI_HOST_CONTROL) | 0x2);
    true
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
        DriverRuntimeBusLinkDescriptor, DriverRuntimeFramebufferDescriptor,
        DriverRuntimeResourceRangeDescriptor, DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
        DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE, DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT,
        DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE, DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
        DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS, DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER,
        DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED, DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
        DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS, DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
        DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS, DRIVER_RUNTIME_RESOURCE_KIND_DMA,
        DRIVER_RUNTIME_RESOURCE_KIND_MMIO, DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
        DRIVER_RUNTIME_RESOURCE_PAGE_BYTES, DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL,
        DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA, DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
        DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST, DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
        DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL, DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI,
    };

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock()
            .expect("runtime tests must serialize global state")
    }

    fn reset_runtime_for_test() {
        reset_test_ring();
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
        SDIO_RUNTIME_FLAGS.store(0, Ordering::Release);
        SDIO_CMD_COUNT.store(0, Ordering::Release);
        PCIE_RUNTIME_FLAGS.store(0, Ordering::Release);
        PCIE_OP_COUNT.store(0, Ordering::Release);
        GENET_RUNTIME_STATE.with_mut(GenetRuntimeState::reset);
        PCIE_RUNTIME_STATE.with_mut(PcieRuntimeState::reset);
        USB_RUNTIME_STATE.with_mut(UsbRuntimeState::reset);
        SDIO_RUNTIME_STATE.with_mut(SdioRuntimeState::reset);
        CYW43_RUNTIME_STATE.with_mut(Cyw43RuntimeState::reset);
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
            HOT_PATH_CYW43_WIFI => (
                CYW43_REQUIRED_MMIO_PAGES,
                CYW43_REQUIRED_DMA_PAGES,
                CYW43_REQUIRED_SHARED_PAGES,
            ),
            HOT_PATH_SDIO_HOST => (
                SDIO_REQUIRED_MMIO_PAGES,
                SDIO_REQUIRED_DMA_PAGES,
                SDIO_REQUIRED_SHARED_PAGES,
            ),
            HOT_PATH_PCIE_ROOT => (PCIE_REQUIRED_MMIO_PAGES, 0, PCIE_REQUIRED_SHARED_PAGES),
            _ => (1, 0, 1),
        };
        let mmio_descriptor_pages =
            mmio_pages.min(pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES as u16);
        let dma_descriptor_pages =
            dma_pages.min(pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_DMA_PAGES as u16);
        let shared_descriptor_pages =
            shared_pages.min(pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES as u16);
        descriptor.mmio_page_count = mmio_descriptor_pages;
        descriptor.dma_page_count = dma_descriptor_pages;
        descriptor.shared_page_count = shared_descriptor_pages;
        for index in 0..usize::from(mmio_descriptor_pages) {
            descriptor.mmio_pages[index] =
                pi4_driver_abi::DriverRuntimePageDescriptor::new(0x1000_0000 + index * 0x1000);
        }
        for index in 0..usize::from(dma_descriptor_pages) {
            descriptor.dma_pages[index] =
                pi4_driver_abi::DriverRuntimePageDescriptor::new(0x2000_0000 + index * 0x1000);
        }
        for index in 0..usize::from(shared_descriptor_pages) {
            descriptor.shared_pages[index] =
                pi4_driver_abi::DriverRuntimePageDescriptor::new(0x4000_0000 + index * 0x1000);
        }
        let mut range_index = 0usize;
        if mmio_pages != 0 {
            descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED;
            descriptor.resource_ranges[range_index] = DriverRuntimeResourceRangeDescriptor::new(
                DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
                DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                    | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                    | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
                match hot_path {
                    HOT_PATH_USB_KEYBOARD => DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI,
                    HOT_PATH_GENET_NIC => DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
                    HOT_PATH_CYW43_WIFI => DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
                    HOT_PATH_SDIO_HOST => DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
                    HOT_PATH_PCIE_ROOT => DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST,
                    _ => pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_GENERIC,
                },
                DRIVER_TASK_DEVICE_MMIO_VADDR as u64,
                0x1000_0000,
                u64::from(mmio_pages) * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
                mmio_pages,
                0,
            );
            range_index += 1;
        }
        if dma_pages != 0 {
            descriptor.resource_ranges[range_index] = DriverRuntimeResourceRangeDescriptor::new(
                DRIVER_RUNTIME_RESOURCE_KIND_DMA,
                DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                    | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                    | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
                DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
                DRIVER_TASK_DMA_BUFFER_VADDR as u64,
                0x2000_0000,
                u64::from(dma_pages) * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
                dma_pages,
                0,
            );
            range_index += 1;
        }
        if shared_pages != 0 {
            descriptor.resource_ranges[range_index] = DriverRuntimeResourceRangeDescriptor::new(
                DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                    | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE
                    | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
                if hot_path == HOT_PATH_CYW43_WIFI {
                    DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL
                } else {
                    DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL
                },
                DRIVER_TASK_SHARED_BUFFER_VADDR as u64,
                0x4000_0000,
                u64::from(shared_pages) * DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
                shared_pages,
                0,
            );
            range_index += 1;
        }
        descriptor.resource_range_count = range_index as u16;
        match hot_path {
            HOT_PATH_USB_KEYBOARD => {
                descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS;
                descriptor.bus_link_count = 1;
                descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
                    HOT_PATH_PCIE_ROOT,
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE,
                    0,
                    DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as u32,
                    DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
                );
            }
            HOT_PATH_CYW43_WIFI => {
                descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS;
                descriptor.bus_link_count = 1;
                descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
                    HOT_PATH_SDIO_HOST,
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
                    0,
                    DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as u32,
                    DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
                );
            }
            _ => {}
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

    fn stage_bytes(offset: usize, bytes: &[u8]) {
        for (index, byte) in bytes.iter().copied().enumerate() {
            write_ring_byte(offset + index, byte);
        }
    }

    fn stage_u16(offset: usize, value: u16) {
        stage_bytes(offset, &value.to_le_bytes());
    }

    fn stage_u32(offset: usize, value: u32) {
        stage_bytes(offset, &value.to_le_bytes());
    }

    fn stage_cyw43_descriptor(desc: DriverRuntimeCyw43CommandDescriptor) {
        let offset = DRIVER_TASK_RING_FRAME_OFFSET;
        stage_u16(offset, desc.op);
        stage_u16(offset + 2, desc.flags);
        stage_u32(offset + 4, desc.target_addr);
        stage_u16(offset + 8, desc.payload_offset);
        stage_u16(offset + 10, desc.payload_len);
        stage_u32(offset + 12, desc.total_len);
        stage_u32(offset + 16, desc.arg0);
        stage_u32(offset + 20, desc.arg1);
        stage_u32(offset + 24, desc.reserved);
    }

    fn cyw43_descriptor_command(sequence: u32) -> DriverTaskCommandRecord {
        DriverTaskCommandRecord {
            sequence,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_CYW43_WIFI,
            arg1: ROLE_NET,
            aux0: DRIVER_RUNTIME_CYW43_COMMAND_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>() as u16,
                flags: 0,
            },
        }
    }

    fn cyw43_runtime_payload_offset() -> u16 {
        (DRIVER_TASK_RING_FRAME_OFFSET
            + core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>()) as u16
    }

    fn init_cyw43_engine_for_test() {
        init_runtime_for_test(HOT_PATH_CYW43_WIFI, ROLE_NET);
        let init = DriverTaskCommandRecord {
            sequence: 70,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_CYW43_WIFI,
            arg1: ROLE_NET,
            aux0: DRIVER_RUNTIME_NET_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, init),
            DriverTaskCompletionRecord::progress(70, 1)
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
    fn hardware_work_fails_closed_before_runtime_init() {
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
    fn cyw43_runtime_streams_firmware_releases_and_transmits_over_sdio() {
        let _guard = test_guard();
        reset_runtime_for_test();
        init_cyw43_engine_for_test();
        let payload_offset = cyw43_runtime_payload_offset();
        let payload = [
            0x55, 0xaa, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xa0,
        ];
        stage_bytes(usize::from(payload_offset), &payload);
        stage_cyw43_descriptor(DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            flags: 0,
            target_addr: CYW43_RAM_BASE_4345,
            payload_offset,
            payload_len: payload.len() as u16,
            total_len: payload.len() as u32,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        });
        assert_eq!(
            service_command(0, cyw43_descriptor_command(71)),
            DriverTaskCompletionRecord::progress(71, payload.len() as u32)
        );

        let nvram = b"boardrev=0xa020d3\0";
        stage_bytes(usize::from(payload_offset), nvram);
        stage_cyw43_descriptor(DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
            flags: 0,
            target_addr: CYW43_RAM_BASE_4345 + 0x1000,
            payload_offset,
            payload_len: nvram.len() as u16,
            total_len: nvram.len() as u32,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        });
        assert_eq!(
            service_command(0, cyw43_descriptor_command(72)),
            DriverTaskCompletionRecord::progress(72, nvram.len() as u32)
        );

        stage_cyw43_descriptor(DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL,
            flags: 0,
            target_addr: CYW43_RAM_BASE_4345 + CYW43_RAM_SIZE_4345_PI4 - 4,
            payload_offset: 0,
            payload_len: 0,
            total_len: 0,
            arg0: 0xfeed_beef,
            arg1: 0,
            reserved: 0,
        });
        assert_eq!(
            service_command(0, cyw43_descriptor_command(73)),
            DriverTaskCompletionRecord::progress(73, 4)
        );

        stage_cyw43_descriptor(DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_RELEASE,
            flags: 0,
            target_addr: 0,
            payload_offset: 0,
            payload_len: 0,
            total_len: 0,
            arg0: CYW43_RAM_BASE_4345,
            arg1: 0,
            reserved: 0,
        });
        assert_eq!(
            service_command(0, cyw43_descriptor_command(74)),
            DriverTaskCompletionRecord::progress(74, 1)
        );

        stage_bytes(usize::from(payload_offset), b"ethernet-frame");
        stage_cyw43_descriptor(DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_ETH_TX,
            flags: 0,
            target_addr: 0,
            payload_offset,
            payload_len: 14,
            total_len: 14,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        });
        assert_eq!(
            service_command(0, cyw43_descriptor_command(75)),
            DriverTaskCompletionRecord::progress(
                75,
                (CYW43_SDPCM_HEADER_BYTES + CYW43_BDC_HEADER_BYTES + 14) as u32
            )
        );
        assert_eq!(
            read_ring_u16(DRIVER_TASK_RING_FRAME_OFFSET),
            (CYW43_SDPCM_HEADER_BYTES + CYW43_BDC_HEADER_BYTES + 14) as u16
        );
        assert_eq!(read_ring_byte(DRIVER_TASK_RING_FRAME_OFFSET + 5) & 0x0f, 2);

        stage_cyw43_descriptor(DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
            flags: 0,
            target_addr: 0,
            payload_offset,
            payload_len: 4,
            total_len: 4,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        });
        assert_eq!(
            service_command(0, cyw43_descriptor_command(76)),
            DriverTaskCompletionRecord::progress(76, (CYW43_SDPCM_HEADER_BYTES + 4) as u32)
        );

        stage_cyw43_descriptor(DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_RX_POLL,
            flags: 0,
            target_addr: 0,
            payload_offset: 0,
            payload_len: 0,
            total_len: 0,
            arg0: 0,
            arg1: 0,
            reserved: 0,
        });
        assert_eq!(
            service_command(0, cyw43_descriptor_command(77)),
            DriverTaskCompletionRecord::idle(77)
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
    fn sdio_descriptor_helpers_encode_cmd52_and_cmd53_arguments() {
        assert_eq!(
            sdio_cmd52_arg(false, 1, 0x1234, 0),
            (1 << 28) | (0x1234 << 9)
        );
        assert_eq!(
            sdio_cmd52_arg(true, 2, 0x20, 0x5a),
            (1 << 31) | (2 << 28) | (0x20 << 9) | 0x5a
        );
        assert_eq!(
            sdio_cmd53_arg(true, 2, 0x1_0000, true, 4, 0),
            (1 << 31) | (2 << 28) | (1 << 27) | (1 << 26) | (0x1_0000 << 9) | 4
        );
        assert_eq!(
            sdio_descriptor_response_flags(DRIVER_RUNTIME_SDIO_RESP_SHORT_BUSY),
            DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY
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
            vaddr: DRIVER_RUNTIME_FRAMEBUFFER_VADDR,
            paddr: 0x3000_0000,
            width: 640,
            height: 480,
            pitch: 640 * 4,
            format: DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
        };
        let range_index = descriptor.resource_range_count as usize;
        descriptor.resource_ranges[range_index] = DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE
                | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
            DRIVER_RUNTIME_RESOURCE_TAG_HDMI_FRAMEBUFFER,
            DRIVER_RUNTIME_FRAMEBUFFER_VADDR,
            0x3000_0000,
            DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
            1,
            0,
        );
        descriptor.resource_range_count += 1;
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
        let engine_init = DriverTaskCommandRecord {
            sequence: 31,
            opcode: OPCODE_SERVICE,
            flags: 0,
            arg0: HOT_PATH_HDMI_TEXT,
            arg1: ROLE_DISPLAY,
            aux0: DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX,
            aux1: 0,
            budget: budget(),
            frame: DriverFrameDescriptor::empty(),
        };
        assert_eq!(
            service_command(0, engine_init),
            DriverTaskCompletionRecord::progress(31, 1)
        );
        let frame = DriverTaskCommandRecord {
            sequence: 32,
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
            DriverTaskCompletionRecord::progress(32, 5)
        );
    }
}
