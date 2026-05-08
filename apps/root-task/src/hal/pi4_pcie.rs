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
const PCIE_HARD_DEBUG_SERDES_IDDQ_MASK: u32 = 0x08000000;
const PCIE_RGR1_SW_INIT_1_INIT_MASK: u32 = 0x2;
const PCIE_RGR1_SW_INIT_1_PERST_MASK: u32 = 0x1;

const BCM2711_PCIE_STATUS_PORT: u32 = 0x80;
const BCM2711_PCIE_STATUS_DL_ACTIVE: u32 = 0x20;
const BCM2711_PCIE_STATUS_PHY_LINK_UP: u32 = 0x10;

const VL805_PCI_DEV_ADDR: u32 = 0x0010_0000;
const VL805_PCI_VENDOR_ID: u16 = 0x1106;
const VL805_PCI_DEVICE_ID: u16 = 0x3483;
const VL805_EXPECTED_CLASS_CODE: u32 = 0x000c_0330;

const PCI_CFG_VENDOR_DEVICE: usize = 0x00;
const PCI_CFG_COMMAND_STATUS: usize = 0x04;
const PCI_CFG_CLASS_REVISION: usize = 0x08;
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
const PCI_CAP_NEXT_MASK: u8 = 0xfc;
const PCI_CAP_TRAVERSE_LIMIT: usize = 16;
const PCI_MSI_CONTROL_OFFSET: usize = 2;
const PCI_MSI_CONTROL_ENABLE: u16 = 1;

const RPI4_VL805_XHCI_MMIO: usize = 0x0000_0006_0000_0000;
const RPI4_PCIE_BUS_MMIO_WINDOW_BASE: usize = 0xC000_0000;
const RPI4_PCIE_BUS_MMIO_WINDOW_BASE_U32: u32 = 0xC000_0000;
const RPI4_PCIE_CPU_MMIO_WINDOW_BASE: usize = RPI4_VL805_XHCI_MMIO;
const RPI4_PCIE_BUS_MMIO_WINDOW_BYTES: usize = 0x4000_0000;
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
const PCIE_EXT_CFG_SELECT_SETTLE_SPINS: usize = 1_024;
const PCIE_EXT_CFG_SELECTOR_RETRIES: usize = 2;
const VL805_XHCI_PORTSC_BASE_OFFSET: usize = 0x420;
const VL805_XHCI_PORTSC_STRIDE: usize = 0x10;
const VL805_XHCI_PORT_REGISTER_MMIO_LIMIT: usize = 0x1_0000;

static PCIE_STATUS_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_EXT_DATA_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_EXT_INDEX_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_ROOT_INIT_ATTEMPTED: AtomicUsize = AtomicUsize::new(0);
static PCIE_ROOT_INIT_POST_MAILBOX_ATTEMPTED: AtomicUsize = AtomicUsize::new(0);
static PCIE_LINK_AND_RC_READY_PROVEN: AtomicUsize = AtomicUsize::new(0);

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

    fn root_init_latch(self) -> &'static AtomicUsize {
        match self {
            Self::Initial => &PCIE_ROOT_INIT_ATTEMPTED,
            Self::PostMailboxReset => &PCIE_ROOT_INIT_POST_MAILBOX_ATTEMPTED,
        }
    }
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
}

impl Pi4Vl805PcieProof {
    #[must_use]
    pub const fn msi_disabled(self) -> bool {
        match self.msi_control_after {
            Some(control) => vl805_msi_control_disabled(control),
            None => false,
        }
    }
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
pub const fn vl805_post_mailbox_ext_cfg_retry_needed(
    mmio: usize,
    high_bar_mmio: usize,
    fresh_runtime_ready: bool,
    runtime_touch_enabled: bool,
) -> bool {
    runtime_touch_enabled && mmio == high_bar_mmio && !fresh_runtime_ready
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
    fence(Ordering::SeqCst);
    let ptr = addr as *mut u32;
    // SAFETY: the caller-installed xHCI hook supplies a live device mapping
    // for the VL805 MMIO window, and the offset was bounded to the root-port
    // register aperture before this volatile device write.
    unsafe { ptr::write_volatile(ptr, value) };
    fence(Ordering::SeqCst);
}

/// Flushes posted VL805 xHCI MMIO writes through a HAL-owned PCI config read.
pub fn vl805_xhci_flush_posted_write(mmio_virt: usize, offset: usize, value: u32, stage: u16) {
    let config_page = PCIE_EXT_DATA_PAGE_VIRT.load(Ordering::Acquire);
    let index_page = PCIE_EXT_INDEX_PAGE_VIRT.load(Ordering::Acquire);
    if config_page == 0 || index_page == 0 {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 posted-write flush skipped stage=0x{stage:04x} offset=0x{offset:04x} value=0x{value:08x} reason=no-ext-cfg mmio=0x{mmio_virt:016x}"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return;
    }
    let Ok(config_virt) = same_page_reg_virt(config_page, BCM2711_PCIE_EXT_CFG_DATA) else {
        boot_log::force_uart_line(
            "[local-seat] vl805 posted-write flush skipped reason=bad-ext-cfg-data",
        );
        return;
    };
    let Ok(index_reg) = same_page_reg_virt(index_page, BCM2711_PCIE_EXT_CFG_INDEX) else {
        boot_log::force_uart_line(
            "[local-seat] vl805 posted-write flush skipped reason=bad-ext-cfg-index",
        );
        return;
    };
    fence(Ordering::SeqCst);
    let selected = bcm2711_ext_cfg_select(index_reg);
    let command_status = pci_cfg_read_u32(config_virt, PCI_CFG_COMMAND_STATUS);
    fence(Ordering::SeqCst);
    let mut line = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] vl805 posted-write flush stage=0x{stage:04x} offset=0x{offset:04x} value=0x{value:08x} selected=0x{selected:08x} cmdstat=0x{command_status:08x} source=hal-ext-cfg"
        ),
    );
    boot_log::force_uart_line(line.as_str());
}

fn prove_pi4_vl805_pcie_ownership(
    hal: &mut KernelHal<'_>,
    phase: Pi4PcieProofPhase,
) -> Result<Pi4Vl805PcieProof, HalError> {
    pi4_wifi::power_on_vl805_usb_hcd(hal)?;

    let status_page =
        map_pcie_reg_page_cached(hal, BCM2711_PCIE_MISC_PCIE_STATUS, "pi4-pcie-status")?;
    let status_reg = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_PCIE_STATUS)?;
    mask_and_clear_pcie_irq_sources(status_page);

    // seL4 device untyped retyping is monotonic. Map the BCM2711 PCIe register
    // pages in ascending physical order so root init's SW_INIT page does not
    // consume past the lower EXT_CFG_DATA page before the exact VL805 proof.
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
                "[local-seat] vl805 bcm2711-pcie status inconclusive status=0x{status:08x} action=exact-vl805-ext-cfg-proof"
            ),
        );
        boot_log::force_uart_line(line.as_str());
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
        vendor_id = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_VENDOR_DEVICE);
        device_id = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_VENDOR_DEVICE + 2);
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

    let class_revision = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_CLASS_REVISION);
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

    let command_before = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS);
    let mut bar0 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR0);
    let mut bar1 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR1);
    if status_ready && vl805_bar_assignment_needed(bar0, bar1) {
        let assigned_bar0 = vl805_pi4_assigned_bar0_value();
        vl805_cfg_write_u32(index_reg, config_virt, PCI_CFG_BAR1, 0);
        vl805_cfg_write_u32(index_reg, config_virt, PCI_CFG_BAR0, assigned_bar0);
        fence(Ordering::SeqCst);
        pcie_spin_delay(PCIE_EXT_CFG_SELECT_SETTLE_SPINS);
        let reassigned_bar0 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR0);
        let reassigned_bar1 = vl805_cfg_read_u32(index_reg, config_virt, PCI_CFG_BAR1);
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
    let exact_config_ready =
        vl805_exact_config_tuple_ready(vendor_device, class_revision, bar0, bar1);
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

    if !status_ready {
        if !exact_config_ready {
            return Err(HalError::Unsupported("pcie-link-or-rc-not-ready"));
        }
        configure_pi4_pcie_outbound_window(status_page)?;
        PCIE_LINK_AND_RC_READY_PROVEN.store(1, Ordering::Release);
        let mut line = heapless::String::<240>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie ext-cfg exact-proof promoted status=0x{status:08x} id={vendor_id:04x}:{device_id:04x} class=0x{class_code:06x} bar0=0x{bar0:08x}"
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    let command_masked = vl805_poll_only_intx_mask_command(command_before);
    if command_masked != command_before {
        vl805_cfg_write_u16(
            index_reg,
            config_virt,
            PCI_CFG_COMMAND_STATUS,
            command_masked,
        );
    }
    let command_masked_after = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS);
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

    let (msi_control_before, msi_control_after) =
        disable_vl805_msi_for_poll_only(index_reg, config_virt)
            .ok_or(HalError::Unsupported("vl805-msi"))?;

    let command_required = vl805_poll_only_bus_master_command(command_masked_after);
    if command_required != command_masked_after {
        vl805_cfg_write_u16(
            index_reg,
            config_virt,
            PCI_CFG_COMMAND_STATUS,
            command_required,
        );
    }
    let command_after = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS);
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
        msi_control_before: Some(msi_control_before),
        msi_control_after: Some(msi_control_after),
    })
}

fn ensure_pi4_pcie_root_ready(
    hal: &mut KernelHal<'_>,
    status_page: usize,
    status_reg: usize,
    phase: Pi4PcieProofPhase,
) -> Result<u32, HalError> {
    let status_before = mmio_read_u32(status_reg);
    if remember_pi4_pcie_link_and_rc_ready(status_before) {
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
    let misc_ctrl = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_MISC_CTRL)?;
    let hard_debug = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_HARD_PCIE_HARD_DEBUG)?;
    let rc_bar1 = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR1_CONFIG_LO)?;
    let rc_bar2_lo = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR2_CONFIG_LO)?;
    let rc_bar2_hi = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR2_CONFIG_HI)?;
    let rc_bar3 = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_RC_BAR3_CONFIG_LO)?;

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
        configure_pi4_pcie_outbound_window(status_page)?;
    }

    let mut done = heapless::String::<320>::new();
    let _ = core::fmt::Write::write_fmt(
            &mut done,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie root-init done stage={} status_before=0x{status_before:08x} status_after=0x{status_after:08x} ready={} polls={polls} post_perst_ms={} poll_window_ms={} poll_interval_ms={} delay_scale={} write_flush=readback",
                phase.label(),
            ready as u8,
            PCIE_POST_PERST_SETTLE_MS,
            PCIE_LINK_POLL_TOTAL_MS,
            PCIE_LINK_POLL_INTERVAL_MS,
            PCIE_ROOT_DELAY_SPIN_SAFETY_MULTIPLIER,
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
    for _ in 0..spins {
        core::hint::spin_loop();
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
    let Some(coverage) = hal.device_coverage(paddr, PAGE_BITS) else {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!("[local-seat] {label} map exact miss paddr=0x{paddr:016x} reason=no-device-coverage"),
        );
        boot_log::force_uart_line(line.as_str());
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
        mmio_write_u32(cpu_mask_set, u32::MAX);
        mmio_write_u32(cpu_clr, u32::MAX);
        mmio_write_u32(msi_mask_set, u32::MAX);
        mmio_write_u32(msi_clr, u32::MAX);
        boot_log::force_uart_line(
            "[local-seat] vl805 bcm2711-pcie irq sources masked source=hal-ext-cfg",
        );
    }
}

#[inline]
const fn pcie_status_link_up_and_rc(status: u32) -> bool {
    (status & BCM2711_PCIE_STATUS_DL_ACTIVE) != 0
        && (status & BCM2711_PCIE_STATUS_PHY_LINK_UP) != 0
        && (status & BCM2711_PCIE_STATUS_PORT) != 0
}

#[inline]
const fn pcie_status_link_bits_present(status: u32) -> bool {
    (status & (BCM2711_PCIE_STATUS_DL_ACTIVE | BCM2711_PCIE_STATUS_PHY_LINK_UP))
        == (BCM2711_PCIE_STATUS_DL_ACTIVE | BCM2711_PCIE_STATUS_PHY_LINK_UP)
}

#[inline]
const fn post_mailbox_ext_cfg_data_read_deferred(status: u32) -> bool {
    !pcie_status_link_bits_present(status)
}

fn disable_vl805_msi_for_poll_only(index_reg: usize, config_virt: usize) -> Option<(u16, u16)> {
    let status = vl805_cfg_read_u16(index_reg, config_virt, PCI_CFG_COMMAND_STATUS + 2);
    if (status & PCI_STATUS_CAPABILITIES_LIST) == 0 {
        boot_log::force_uart_line(
            "[local-seat] vl805 bcm2711-pcie msi proof skipped reason=no-cap-list",
        );
        return None;
    }

    let mut cap =
        (vl805_cfg_read_u8(index_reg, config_virt, PCI_CFG_CAP_PTR) & PCI_CAP_NEXT_MASK) as usize;
    for _ in 0..PCI_CAP_TRAVERSE_LIMIT {
        if !(0x40..0x100).contains(&cap) {
            break;
        }
        let cap_id = vl805_cfg_read_u8(index_reg, config_virt, cap);
        let next =
            (vl805_cfg_read_u8(index_reg, config_virt, cap + 1) & PCI_CAP_NEXT_MASK) as usize;
        if cap_id == PCI_CAP_ID_MSI {
            let ctrl_offset = cap + PCI_MSI_CONTROL_OFFSET;
            let control_before = vl805_cfg_read_u16(index_reg, config_virt, ctrl_offset);
            let control_request = vl805_msi_control_disable_value(control_before);
            if control_request != control_before {
                vl805_cfg_write_u16(index_reg, config_virt, ctrl_offset, control_request);
            }
            let control_after = vl805_cfg_read_u16(index_reg, config_virt, ctrl_offset);
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
            return disabled.then_some((control_before, control_after));
        }
        if next == 0 || next == cap {
            break;
        }
        cap = next;
    }

    boot_log::force_uart_line(
        "[local-seat] vl805 bcm2711-pcie msi proof skipped reason=msi-cap-missing",
    );
    None
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
fn vl805_cfg_read_u8(index_reg: usize, config_virt: usize, offset: usize) -> u8 {
    let _ = bcm2711_ext_cfg_select(index_reg);
    pci_cfg_read_u8(config_virt, offset)
}

#[inline]
fn vl805_cfg_read_u16(index_reg: usize, config_virt: usize, offset: usize) -> u16 {
    let _ = bcm2711_ext_cfg_select(index_reg);
    pci_cfg_read_u16(config_virt, offset)
}

#[inline]
fn vl805_cfg_write_u16(index_reg: usize, config_virt: usize, offset: usize, value: u16) {
    let _ = bcm2711_ext_cfg_select(index_reg);
    pci_cfg_write_u16(config_virt, offset, value);
}

#[inline]
fn vl805_cfg_write_u32(index_reg: usize, config_virt: usize, offset: usize, value: u32) {
    let _ = bcm2711_ext_cfg_select(index_reg);
    pci_cfg_write_u32(config_virt, offset, value);
}

#[inline]
fn vl805_cfg_read_u32(index_reg: usize, config_virt: usize, offset: usize) -> u32 {
    let _ = bcm2711_ext_cfg_select(index_reg);
    pci_cfg_read_u32(config_virt, offset)
}

#[inline]
fn pci_cfg_read_u8(config_virt: usize, offset: usize) -> u8 {
    let Some(addr) = config_virt.checked_add(offset) else {
        return 0xff;
    };
    // SAFETY: `config_virt` is the HAL-owned BCM2711 EXT_CFG_DATA mapping for
    // the selected VL805 function. PCI config byte reads are volatile MMIO.
    unsafe { ptr::read_volatile(addr as *const u8) }
}

#[inline]
fn pci_cfg_read_u16(config_virt: usize, offset: usize) -> u16 {
    let Some(addr) = config_virt.checked_add(offset) else {
        return 0xffff;
    };
    // SAFETY: `config_virt` is the HAL-owned BCM2711 EXT_CFG_DATA mapping for
    // the selected VL805 function. PCI config word reads are volatile MMIO.
    unsafe { ptr::read_volatile(addr as *const u16) }
}

#[inline]
fn pci_cfg_write_u16(config_virt: usize, offset: usize, value: u16) {
    let Some(addr) = config_virt.checked_add(offset) else {
        return;
    };
    // SAFETY: `config_virt` is the HAL-owned BCM2711 EXT_CFG_DATA mapping for
    // the selected VL805 function. PCI config word writes are volatile MMIO.
    unsafe {
        ptr::write_volatile(addr as *mut u16, value);
    }
}

#[inline]
fn pci_cfg_read_u32(config_virt: usize, offset: usize) -> u32 {
    let Some(addr) = config_virt.checked_add(offset) else {
        return 0xffff_ffff;
    };
    // SAFETY: `config_virt` is the HAL-owned BCM2711 EXT_CFG_DATA mapping for
    // the selected VL805 function. PCI config dword reads are volatile MMIO.
    unsafe { ptr::read_volatile(addr as *const u32) }
}

#[inline]
fn pci_cfg_write_u32(config_virt: usize, offset: usize, value: u32) {
    let Some(addr) = config_virt.checked_add(offset) else {
        return;
    };
    // SAFETY: `config_virt` is the HAL-owned BCM2711 EXT_CFG_DATA mapping for
    // the selected VL805 function. PCI config dword writes are volatile MMIO.
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
        assert!(pcie_status_link_bits_present(
            BCM2711_PCIE_STATUS_DL_ACTIVE | BCM2711_PCIE_STATUS_PHY_LINK_UP
        ));
        assert!(!pcie_status_link_bits_present(BCM2711_PCIE_STATUS_PORT));
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
        assert!(!post_mailbox_ext_cfg_data_read_deferred(
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
        let (status_page, _) = pcie_reg_page(BCM2711_PCIE_MISC_PCIE_STATUS).expect("status page");
        let (ext_data_page, _) = pcie_reg_page(BCM2711_PCIE_EXT_CFG_DATA).expect("ext data page");
        let (ext_index_page, _) =
            pcie_reg_page(BCM2711_PCIE_EXT_CFG_INDEX).expect("ext index page");
        let (sw_init_page, _) = pcie_reg_page(BCM2711_PCIE_RGR1_SW_INIT_1).expect("sw init page");

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
    fn vl805_msi_control_disable_clears_enable_bit_only() {
        assert_eq!(vl805_msi_control_disable_value(0x00a5), 0x00a4);
        assert!(vl805_msi_control_disabled(0x00a4));
        assert!(!vl805_msi_control_disabled(0x00a5));
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
        assert_eq!(
            replace_u32_field(
                0,
                PCIE_MISC_RC_BAR2_CONFIG_LO_SIZE_MASK,
                brcm_pcie_encode_ibar_size(dma_size),
            ),
            17
        );
        assert_eq!(
            replace_u32_field(0, PCIE_MISC_MISC_CTRL_SCB0_SIZE_MASK, 17),
            0x8800_0000
        );
        assert_eq!(RPI4_PCIE_DMA_BUS_BASE, 0x4_0000_0000);
        assert_eq!(RPI4_PCIE_DMA_CPU_BASE, 0);
    }
}
