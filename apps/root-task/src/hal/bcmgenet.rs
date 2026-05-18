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
        for page in 0..BCMGENET_MMIO_PAGE_COUNT {
            let offset = page
                .checked_mul(PAGE_SIZE)
                .ok_or(HalError::Unsupported("bcmgenet-register-offset-overflow"))?;
            let paddr = candidate
                .checked_add(offset)
                .ok_or(HalError::Unsupported("bcmgenet-register-address-overflow"))?;
            let frame = hal.map_device(paddr).map_err(|err| {
                warn!(
                    "[bcmgenet-hal] map_device failed mmio=0x{:016x} page={} err={}",
                    candidate, page, err
                );
                err
            })?;
            regs.push(frame)
                .map_err(|_| HalError::Unsupported("bcmgenet-register-page-capacity"))?;
        }

        return BcmGenetRegisters::new(candidate, regs);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sel4::{DeviceCoverage, DeviceFrame, KernelEnvSnapshot, RamFrame};

    struct CoverageOnlyHal {
        covered_base: usize,
        covered_pages: usize,
    }

    impl DeviceHal for CoverageOnlyHal {
        type Error = HalError;

        fn map_device(&mut self, _paddr: usize) -> Result<DeviceFrame, Self::Error> {
            Err(HalError::Unsupported("test-map-device-unused"))
        }

        fn alloc_dma_frame(&mut self) -> Result<RamFrame, Self::Error> {
            Err(HalError::Unsupported("test-dma-unused"))
        }

        fn reserve_dma_guard_page(&mut self) -> Result<usize, Self::Error> {
            Err(HalError::Unsupported("test-guard-unused"))
        }

        fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
            if size_bits != PAGE_BITS {
                return None;
            }
            let covered_end = self
                .covered_base
                .checked_add(self.covered_pages.checked_mul(PAGE_SIZE)?)?;
            if (self.covered_base..covered_end).contains(&paddr) {
                Some(DeviceCoverage {
                    base: self.covered_base,
                    limit: covered_end,
                    size_bits: PAGE_BITS as u8,
                    index: 0,
                    used: false,
                })
            } else {
                None
            }
        }

        fn snapshot(&self) -> KernelEnvSnapshot {
            panic!("GENET HAL coverage tests do not use snapshots")
        }
    }

    #[test]
    fn genet_dma_policy_is_physical_uncached_for_pi4() {
        assert!(dma_uncached());
        assert_eq!(dma_address_policy_name(), "physical");
        assert_eq!(dma_bus_addr(0x1234_5000), 0x1234_5000);
        assert_eq!(dma_bus_addr(DMA_ALIAS_WINDOW_BYTES), DMA_ALIAS_WINDOW_BYTES);
    }

    #[test]
    fn genet_candidate_requires_all_register_pages_covered() {
        let full = CoverageOnlyHal {
            covered_base: GENET_MMIO_CANDIDATES[0],
            covered_pages: BCMGENET_MMIO_PAGE_COUNT,
        };
        assert!(candidate_covered(&full, GENET_MMIO_CANDIDATES[0]));

        let partial = CoverageOnlyHal {
            covered_base: GENET_MMIO_CANDIDATES[0],
            covered_pages: BCMGENET_MMIO_PAGE_COUNT - 1,
        };
        assert!(!candidate_covered(&partial, GENET_MMIO_CANDIDATES[0]));
    }

    struct MapFailHal {
        map_calls: usize,
    }

    impl DeviceHal for MapFailHal {
        type Error = HalError;

        fn map_device(&mut self, _paddr: usize) -> Result<DeviceFrame, Self::Error> {
            self.map_calls += 1;
            Err(HalError::Unsupported("test-map-device-fail"))
        }

        fn alloc_dma_frame(&mut self) -> Result<RamFrame, Self::Error> {
            Err(HalError::Unsupported("test-dma-unused"))
        }

        fn reserve_dma_guard_page(&mut self) -> Result<usize, Self::Error> {
            Err(HalError::Unsupported("test-guard-unused"))
        }

        fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
            if size_bits != PAGE_BITS {
                return None;
            }
            for base in GENET_MMIO_CANDIDATES {
                let covered_end = base.checked_add(BCMGENET_MMIO_PAGE_COUNT * PAGE_SIZE)?;
                if (base..covered_end).contains(&paddr) {
                    return Some(DeviceCoverage {
                        base,
                        limit: covered_end,
                        size_bits: PAGE_BITS as u8,
                        index: 0,
                        used: false,
                    });
                }
            }
            None
        }

        fn snapshot(&self) -> KernelEnvSnapshot {
            panic!("GENET HAL mapping tests do not use snapshots")
        }
    }

    #[test]
    fn genet_covered_candidate_mapping_failure_fails_closed_without_alias_retry() {
        let mut hal = MapFailHal { map_calls: 0 };

        let err = match map_registers(&mut hal) {
            Ok(_) => panic!("mapping failure should surface"),
            Err(err) => err,
        };

        assert_eq!(err, HalError::Unsupported("test-map-device-fail"));
        assert_eq!(
            hal.map_calls, 1,
            "a covered GENET aperture with a mapping failure should not consume additional aliases"
        );
    }
}
