// Author: Lukas Bower

//! Platform-agnostic PCI data model used by the HAL to describe discovered devices.

use core::fmt;

/// Maximum number of BARs exposed by a conventional PCI function.
pub const PCI_MAX_BARS: usize = 6;

/// Unique identifier for a PCI function.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PciAddress {
    pub segment: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    /// Constructs a new PCI address.
    #[must_use]
    pub const fn new(segment: u16, bus: u8, device: u8, function: u8) -> Self {
        Self {
            segment,
            bus,
            device,
            function,
        }
    }
}

/// BAR interpretation captured by the HAL.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PciBarKind {
    Mmio32,
    Mmio64,
    IoPort,
}

/// PCI BAR description advertised to drivers.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PciBar {
    pub index: u8,
    pub kind: PciBarKind,
    pub base: u64,
    pub size: u64,
}

impl PciBar {
    /// Returns true when the BAR describes an MMIO region.
    #[must_use]
    pub const fn is_mmio(&self) -> bool {
        matches!(self.kind, PciBarKind::Mmio32 | PciBarKind::Mmio64)
    }
}

/// PCI function metadata populated by the HAL during discovery.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PciDeviceInfo {
    pub addr: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub bars: [Option<PciBar>; PCI_MAX_BARS],
}

/// Immutable collection of discovered PCI devices.
pub struct PciTopology {
    pub devices: &'static [PciDeviceInfo],
}

impl PciTopology {
    /// Returns the first device matching the supplied vendor and device identifiers.
    #[must_use]
    pub fn find_by_vendor_device(&self, vendor_id: u16, device_id: u16) -> Option<&PciDeviceInfo> {
        self.devices
            .iter()
            .find(|device| device.vendor_id == vendor_id && device.device_id == device_id)
    }
}

impl fmt::Debug for PciTopology {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PciTopology")
            .field("device_count", &self.devices.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device() -> PciDeviceInfo {
        PciDeviceInfo {
            addr: PciAddress::new(0, 0, 1, 0),
            vendor_id: 0x10ec,
            device_id: 0x8139,
            class_code: 0x02,
            subclass: 0x00,
            prog_if: 0x00,
            bars: [
                Some(PciBar {
                    index: 0,
                    kind: PciBarKind::Mmio32,
                    base: 0x3000_0000,
                    size: 0x100,
                }),
                None,
                None,
                None,
                None,
                None,
            ],
        }
    }

    #[test]
    fn topology_find_by_id_matches_expected_device() {
        let device = sample_device();
        let topology = PciTopology {
            devices: core::slice::from_ref(&device),
        };

        let found = topology.find_by_vendor_device(0x10ec, 0x8139);
        assert!(found.is_some());
        let dev = found.unwrap();
        assert_eq!(dev.addr.bus, 0);
        assert_eq!(dev.bars[0].unwrap().kind, PciBarKind::Mmio32);
    }

    #[test]
    fn topology_returns_none_for_missing_device() {
        let topology = PciTopology { devices: &[] };
        assert!(topology.find_by_vendor_device(0x1234, 0x5678).is_none());
    }
}
