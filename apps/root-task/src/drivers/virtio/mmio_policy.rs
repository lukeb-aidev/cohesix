// Author: Lukas Bower

//! Virtio MMIO version policy helpers.
//!
//! Modern virtio-mmio v2 devices are accepted by default. Legacy v1 devices
//! require the `virtio-mmio-legacy` feature flag to opt-in explicitly to ensure
//! we do not regress into mixed v1/v2 configurations on macOS/QEMU.

/// Magic value advertised by virtio-mmio devices.
pub const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;
/// Legacy virtio-mmio version.
pub const VIRTIO_MMIO_VERSION_LEGACY: u32 = 1;
/// Modern virtio-mmio version that should be used by default.
pub const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
/// Device identifier for virtio-net.
pub const VIRTIO_DEVICE_ID_NET: u32 = 1;

/// Modes supported by the driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioMmioMode {
    Modern,
    Legacy,
}

/// Evaluation result for a probed virtio-mmio header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtioMmioPolicyError {
    InvalidMagic(u32),
    WrongDevice(u32),
    UnsupportedVersion(u32),
}

/// Determine which virtio-mmio mode to use based on the header and feature
/// toggles.
pub fn evaluate(
    magic: u32,
    version: u32,
    device_id: u32,
    legacy_enabled: bool,
) -> Result<VirtioMmioMode, VirtioMmioPolicyError> {
    if magic != VIRTIO_MMIO_MAGIC {
        return Err(VirtioMmioPolicyError::InvalidMagic(magic));
    }

    if device_id != VIRTIO_DEVICE_ID_NET {
        return Err(VirtioMmioPolicyError::WrongDevice(device_id));
    }

    match version {
        VIRTIO_MMIO_VERSION_MODERN => Ok(VirtioMmioMode::Modern),
        VIRTIO_MMIO_VERSION_LEGACY => {
            if legacy_enabled {
                Ok(VirtioMmioMode::Legacy)
            } else {
                Err(VirtioMmioPolicyError::UnsupportedVersion(version))
            }
        }
        other => Err(VirtioMmioPolicyError::UnsupportedVersion(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_modern_v2_by_default() {
        assert_eq!(
            evaluate(
                VIRTIO_MMIO_MAGIC,
                VIRTIO_MMIO_VERSION_MODERN,
                VIRTIO_DEVICE_ID_NET,
                false
            )
            .unwrap(),
            VirtioMmioMode::Modern
        );
    }

    #[test]
    fn rejects_legacy_without_feature() {
        assert!(matches!(
            evaluate(
                VIRTIO_MMIO_MAGIC,
                VIRTIO_MMIO_VERSION_LEGACY,
                VIRTIO_DEVICE_ID_NET,
                false
            ),
            Err(VirtioMmioPolicyError::UnsupportedVersion(
                VIRTIO_MMIO_VERSION_LEGACY
            ))
        ));
    }

    #[test]
    fn accepts_legacy_with_feature() {
        assert_eq!(
            evaluate(
                VIRTIO_MMIO_MAGIC,
                VIRTIO_MMIO_VERSION_LEGACY,
                VIRTIO_DEVICE_ID_NET,
                true
            )
            .unwrap(),
            VirtioMmioMode::Legacy
        );
    }

    #[test]
    fn rejects_unknown_versions() {
        assert!(matches!(
            evaluate(VIRTIO_MMIO_MAGIC, 7, VIRTIO_DEVICE_ID_NET, false),
            Err(VirtioMmioPolicyError::UnsupportedVersion(7))
        ));
    }
}
