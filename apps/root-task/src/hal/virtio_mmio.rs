// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Own virtio-mmio platform slot discovery and register mapping.
// Author: Lukas Bower

//! QEMU virtio-mmio slot discovery owned by the HAL boundary.

use crate::hal::{DeviceHal, HalError, MappedRegisterPages};

/// Number of virtio-mmio slots exposed by the QEMU `virt` profile.
pub const VIRTIO_MMIO_SLOTS: usize = 8;

/// Distance in bytes between QEMU `virt` virtio-mmio slots.
pub const VIRTIO_MMIO_STRIDE: usize = 0x200;

/// First QEMU `virt` virtio-mmio physical slot.
pub const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const DEVICE_FRAME_BITS: usize = 12;

/// HAL-owned single-page virtio-mmio register mapping.
pub type VirtioMmioRegisters = MappedRegisterPages<1>;

/// Returns the physical base address for a virtio-mmio slot.
#[must_use]
pub fn slot_paddr(slot: usize) -> Option<usize> {
    if slot >= VIRTIO_MMIO_SLOTS {
        None
    } else {
        VIRTIO_MMIO_BASE.checked_add(slot.checked_mul(VIRTIO_MMIO_STRIDE)?)
    }
}

/// Maps one virtio-mmio slot selected by HAL-owned platform layout.
pub fn map_registers<H>(hal: &mut H, slot: usize) -> Result<VirtioMmioRegisters, HalError>
where
    H: DeviceHal<Error = HalError>,
{
    let paddr = slot_paddr(slot).ok_or(HalError::Unsupported("virtio-mmio-slot-invalid"))?;
    if hal.device_coverage(paddr, DEVICE_FRAME_BITS).is_none() {
        return Err(HalError::Unsupported("virtio-mmio-slot-uncovered"));
    }
    VirtioMmioRegisters::single(hal.map_device(paddr)?)
}
