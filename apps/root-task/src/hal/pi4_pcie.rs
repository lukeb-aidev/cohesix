// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide HAL-owned BCM2711 PCIe/VL805 config proof for Pi 4 USB bring-up.
// Author: Lukas Bower

#![allow(unsafe_code)]

use core::cmp;
use core::ptr;
use core::sync::atomic::{fence, AtomicUsize, Ordering};

use super::{DeviceHal, HalError, KernelHal};
use crate::bootstrap::log as boot_log;
use crate::rust_alloc::vec::Vec;
use crate::sel4::{page_get_address, PAGE_BITS};

const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_MASK: usize = PAGE_SIZE - 1;
const MAP_EXACT_ATTEMPT_CAP: usize = 512;
const EXACT_MAP_LOG_STRIDE: usize = 64;

const BCM2711_PCIE_HOST_PHYS_BASE: usize = 0xFD50_0000;
const BCM2711_PCIE_MISC_PCIE_STATUS: usize = 0x4068;
const BCM2711_PCIE_INTR2_CPU_CLR: usize = 0x4308;
const BCM2711_PCIE_INTR2_CPU_MASK_SET: usize = 0x4310;
const BCM2711_PCIE_MSI_INTR2_CLR: usize = 0x4508;
const BCM2711_PCIE_MSI_INTR2_MASK_SET: usize = 0x4510;
const BCM2711_PCIE_EXT_CFG_DATA: usize = 0x8000;
const BCM2711_PCIE_EXT_CFG_INDEX: usize = 0x9000;

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
const PCI_COMMAND_INTERRUPT_DISABLE: u16 = 1 << 10;
const PCI_STATUS_CAPABILITIES_LIST: u16 = 1 << 4;
const PCI_CAP_ID_MSI: u8 = 0x05;
const PCI_CAP_NEXT_MASK: u8 = 0xfc;
const PCI_CAP_TRAVERSE_LIMIT: usize = 16;
const PCI_MSI_CONTROL_OFFSET: usize = 2;
const PCI_MSI_CONTROL_ENABLE: u16 = 1;

const RPI4_VL805_XHCI_MMIO: usize = 0x0000_0006_0000_0000;
const RPI4_PCIE_BUS_MMIO_WINDOW_BASE: usize = 0xC000_0000;
const RPI4_PCIE_CPU_MMIO_WINDOW_BASE: usize = RPI4_VL805_XHCI_MMIO;
const RPI4_PCIE_BUS_MMIO_WINDOW_BYTES: usize = 0x0010_0000;

static PCIE_STATUS_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_EXT_DATA_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);
static PCIE_EXT_INDEX_PAGE_VIRT: AtomicUsize = AtomicUsize::new(0);

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
        prove_pi4_vl805_pcie_ownership(self)
    }
}

fn prove_pi4_vl805_pcie_ownership(hal: &mut KernelHal<'_>) -> Result<Pi4Vl805PcieProof, HalError> {
    let status_page =
        map_pcie_reg_page_cached(hal, BCM2711_PCIE_MISC_PCIE_STATUS, "pi4-pcie-status")?;
    let status_reg = same_page_reg_virt(status_page, BCM2711_PCIE_MISC_PCIE_STATUS)?;
    mask_and_clear_pcie_irq_sources(status_page);

    let status = mmio_read_u32(status_reg);
    if !pcie_status_link_up_and_rc(status) {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie ext-cfg skipped status=0x{status:08x} reason=link-or-rc-not-ready"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(HalError::Unsupported("pcie-link-or-rc-not-ready"));
    }

    let config_page =
        map_pcie_reg_page_cached(hal, BCM2711_PCIE_EXT_CFG_DATA, "pi4-pcie-ext-data")?;
    let index_page =
        map_pcie_reg_page_cached(hal, BCM2711_PCIE_EXT_CFG_INDEX, "pi4-pcie-ext-index")?;
    let config_virt = same_page_reg_virt(config_page, BCM2711_PCIE_EXT_CFG_DATA)?;
    let index_reg = same_page_reg_virt(index_page, BCM2711_PCIE_EXT_CFG_INDEX)?;

    let mut select = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut select,
        format_args!(
            "[local-seat] vl805 bcm2711-pcie ext-cfg mapped source=hal status=0x{status:08x} action=select-device"
        ),
    );
    boot_log::force_uart_line(select.as_str());

    mmio_write_u32(index_reg, VL805_PCI_DEV_ADDR);
    fence(Ordering::SeqCst);

    let vendor_device = pci_cfg_read_u32(config_virt, PCI_CFG_VENDOR_DEVICE);
    let vendor_id = (vendor_device & 0xffff) as u16;
    let device_id = ((vendor_device >> 16) & 0xffff) as u16;
    if vendor_id != VL805_PCI_VENDOR_ID || device_id != VL805_PCI_DEVICE_ID {
        let mut line = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie id mismatch got={vendor_id:04x}:{device_id:04x}"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(HalError::Unsupported("vl805-id"));
    }

    let class_revision = pci_cfg_read_u32(config_virt, PCI_CFG_CLASS_REVISION);
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
        return Err(HalError::Unsupported("vl805-class"));
    }

    let command_before = pci_cfg_read_u16(config_virt, PCI_CFG_COMMAND_STATUS);
    let bar0 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR0);
    let bar1 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR1);
    let Some(mmio) = translate_vl805_pci_bar_to_cpu_mmio(bar0, bar1) else {
        let mut line = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 bcm2711-pcie reject bar=0x{bar0:08x}/0x{bar1:08x} reason=no-mmio-bar"
            ),
        );
        boot_log::force_uart_line(line.as_str());
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
        return Err(HalError::Unsupported("vl805-bar"));
    }

    let command_masked = vl805_poll_only_intx_mask_command(command_before);
    if command_masked != command_before {
        pci_cfg_write_u16(config_virt, PCI_CFG_COMMAND_STATUS, command_masked);
    }
    let command_masked_after = pci_cfg_read_u16(config_virt, PCI_CFG_COMMAND_STATUS);
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
        disable_vl805_msi_for_poll_only(config_virt).ok_or(HalError::Unsupported("vl805-msi"))?;

    let command_required = vl805_poll_only_bus_master_command(command_masked_after);
    if command_required != command_masked_after {
        pci_cfg_write_u16(config_virt, PCI_CFG_COMMAND_STATUS, command_required);
    }
    let command_after = pci_cfg_read_u16(config_virt, PCI_CFG_COMMAND_STATUS);
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

fn disable_vl805_msi_for_poll_only(config_virt: usize) -> Option<(u16, u16)> {
    let status = pci_cfg_read_u16(config_virt, PCI_CFG_COMMAND_STATUS + 2);
    if (status & PCI_STATUS_CAPABILITIES_LIST) == 0 {
        boot_log::force_uart_line(
            "[local-seat] vl805 bcm2711-pcie msi proof skipped reason=no-cap-list",
        );
        return None;
    }

    let mut cap = (pci_cfg_read_u8(config_virt, PCI_CFG_CAP_PTR) & PCI_CAP_NEXT_MASK) as usize;
    for _ in 0..PCI_CAP_TRAVERSE_LIMIT {
        if !(0x40..0x100).contains(&cap) {
            break;
        }
        let cap_id = pci_cfg_read_u8(config_virt, cap);
        let next = (pci_cfg_read_u8(config_virt, cap + 1) & PCI_CAP_NEXT_MASK) as usize;
        if cap_id == PCI_CAP_ID_MSI {
            let ctrl_offset = cap + PCI_MSI_CONTROL_OFFSET;
            let control_before = pci_cfg_read_u16(config_virt, ctrl_offset);
            let control_request = vl805_msi_control_disable_value(control_before);
            if control_request != control_before {
                pci_cfg_write_u16(config_virt, ctrl_offset, control_request);
            }
            let control_after = pci_cfg_read_u16(config_virt, ctrl_offset);
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
    masked_command | PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER
}

#[inline]
const fn vl805_command_ownership_ready(command: u16) -> bool {
    (command & (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_INTERRUPT_DISABLE))
        == (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_INTERRUPT_DISABLE)
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
        assert_eq!(translate_vl805_pci_bar_to_cpu_mmio(0xd000_0004, 0), None);
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
            PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_INTERRUPT_DISABLE
        );
        assert!(vl805_command_ownership_ready(
            PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_INTERRUPT_DISABLE
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
}
