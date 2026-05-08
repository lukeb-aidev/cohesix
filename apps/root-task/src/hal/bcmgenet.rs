// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Own Broadcom GENET MMIO discovery and register mapping for Pi4 HAL.
// Author: Lukas Bower

//! Pi4 Broadcom GENET register mapping owned by the HAL boundary.

use heapless::Vec as HeaplessVec;
use log::warn;

use crate::hal::{DeviceHal, HalError, MappedRegisterPages};
use crate::sel4::{DeviceFrame, PAGE_BITS};

/// Number of 4 KiB register pages covered by the GENETv5 register block.
pub const BCMGENET_MMIO_PAGE_COUNT: usize = 6;

/// HAL-owned GENET register mapping.
pub type BcmGenetRegisters = MappedRegisterPages<BCMGENET_MMIO_PAGE_COUNT>;

const PAGE_SIZE: usize = 1 << PAGE_BITS;

// GENETv5 aliases observed across Pi4 firmware/device-tree mappings. Keeping
// these in HAL prevents the driver from owning physical platform layout.
const GENET_MMIO_CANDIDATES: [usize; 3] = [0xFD58_0000, 0x7D58_0000, 0xFE58_0000];

// Pi4 UEFI + bcmgenet on this path expects physical DMA addresses (no VC alias).
const DMA_USE_BUS_ALIAS: bool = false;
const DMA_BUS_ALIAS_BASE: u64 = 0xC000_0000;
const DMA_ALIAS_WINDOW_BYTES: u64 = 0x4000_0000;

/// Returns whether GENET DMA mappings should be uncached.
#[must_use]
pub const fn dma_uncached() -> bool {
    true
}

/// Returns the device-visible DMA address for a HAL-allocated frame.
#[must_use]
pub const fn dma_bus_addr(phys: u64) -> u64 {
    if DMA_USE_BUS_ALIAS && phys < DMA_ALIAS_WINDOW_BYTES {
        phys | DMA_BUS_ALIAS_BASE
    } else {
        phys
    }
}

/// Returns the name of the DMA address policy for diagnostics.
#[must_use]
pub const fn dma_address_policy_name() -> &'static str {
    if DMA_USE_BUS_ALIAS {
        "vc-0xc0000000"
    } else {
        "physical"
    }
}

/// Maps the platform-selected GENET register block.
pub fn map_registers<H>(hal: &mut H) -> Result<BcmGenetRegisters, HalError>
where
    H: DeviceHal<Error = HalError>,
{
    for candidate in GENET_MMIO_CANDIDATES {
        if !candidate_covered(hal, candidate) {
            continue;
        }

        let mut regs: HeaplessVec<DeviceFrame, BCMGENET_MMIO_PAGE_COUNT> = HeaplessVec::new();
        let mut failed = false;
        for page in 0..BCMGENET_MMIO_PAGE_COUNT {
            let Some(offset) = page.checked_mul(PAGE_SIZE) else {
                failed = true;
                break;
            };
            let Some(paddr) = candidate.checked_add(offset) else {
                failed = true;
                break;
            };
            match hal.map_device(paddr) {
                Ok(frame) => regs
                    .push(frame)
                    .map_err(|_| HalError::Unsupported("bcmgenet-register-page-capacity"))?,
                Err(err) => {
                    failed = true;
                    warn!(
                        "[bcmgenet-hal] map_device failed mmio=0x{:016x} page={} err={}",
                        candidate, page, err
                    );
                    break;
                }
            }
        }

        if !failed && regs.len() == BCMGENET_MMIO_PAGE_COUNT {
            return BcmGenetRegisters::new(candidate, regs);
        }

        warn!(
            "[bcmgenet-hal] candidate 0x{:016x} mapping incomplete; trying next alias",
            candidate
        );
    }

    Err(HalError::Unsupported("bcmgenet-mmio-not-covered"))
}

fn candidate_covered<H>(hal: &H, base: usize) -> bool
where
    H: DeviceHal<Error = HalError>,
{
    for page in 0..BCMGENET_MMIO_PAGE_COUNT {
        let Some(offset) = page.checked_mul(PAGE_SIZE) else {
            return false;
        };
        let Some(paddr) = base.checked_add(offset) else {
            return false;
        };
        if hal.device_coverage(paddr, PAGE_BITS).is_none() {
            return false;
        }
    }
    true
}
