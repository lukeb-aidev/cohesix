// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide authenticated reboot backend wiring for the root-task console.
// Author: Lukas Bower

//! Authenticated platform reboot support for root-task console commands.

#[cfg(test)]
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(feature = "kernel")]
use crate::hal::{DeviceHal, HalError, MappedRegisterPages, MappedRegisterWindow};
#[cfg(feature = "kernel")]
use spin::Mutex;

#[cfg(feature = "kernel")]
const BCM2711_PM_BASE: usize = 0xfe10_0000;
#[cfg(feature = "kernel")]
const PM_RSTC_OFFSET: usize = 0x1c;
#[cfg(feature = "kernel")]
const PM_RSTS_OFFSET: usize = 0x20;
#[cfg(feature = "kernel")]
const PM_WDOG_OFFSET: usize = 0x24;
#[cfg(any(feature = "kernel", test))]
const PM_PASSWORD: u32 = 0x5a00_0000;
#[cfg(feature = "kernel")]
const PM_RSTC_WRCFG_MASK: u32 = 0x0000_0030;
#[cfg(feature = "kernel")]
const PM_RSTC_WRCFG_FULL_RESET: u32 = 0x0000_0020;
#[cfg(feature = "kernel")]
const PM_WDOG_RESET_TICKS: u32 = 10;
#[cfg(any(feature = "kernel", test))]
const PM_RSTS_PI_FIRMWARE_PARTITION_MASK: u32 = 0x0000_0555;
#[cfg(any(feature = "kernel", test))]
const PM_RSTS_COHESIX_FASTBOOT_MASK: u32 = 0x00ff_0000;
#[cfg(any(feature = "kernel", test))]
const PM_RSTS_COHESIX_FASTBOOT_MAGIC: u32 = 0x0043_0000;

#[cfg(feature = "kernel")]
static BCM2711_PM_WATCHDOG: Mutex<Option<MappedRegisterWindow>> = Mutex::new(None);

#[cfg(test)]
static TEST_BACKEND_AVAILABLE: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static TEST_REBOOT_REQUESTS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static TEST_REBOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Error returned when the platform cannot schedule a reboot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RebootError {
    /// No platform reset backend is available for this build/profile.
    BackendUnavailable,
    /// The mapped reset registers rejected a bounded access.
    RegisterAccess,
}

impl RebootError {
    /// Stable detail label used in console refusals and audit records.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::BackendUnavailable => "reboot-backend-unavailable",
            Self::RegisterAccess => "reboot-register-access",
        }
    }
}

/// Register the BCM2711 PM/watchdog reset backend through the HAL mapping path.
#[cfg(feature = "kernel")]
pub fn register_bcm2711_pm_watchdog<H>(hal: &mut H) -> Result<(), HalError>
where
    H: DeviceHal<Error = HalError>,
{
    let regs: MappedRegisterPages<1> =
        MappedRegisterPages::single(hal.map_device(BCM2711_PM_BASE)?)?;
    *BCM2711_PM_WATCHDOG.lock() = Some(regs.register_window()?);
    Ok(())
}

/// Return whether a reboot backend is ready for the current profile.
#[must_use]
pub fn backend_available() -> bool {
    #[cfg(test)]
    if TEST_BACKEND_AVAILABLE.load(Ordering::SeqCst) {
        return true;
    }

    #[cfg(feature = "kernel")]
    {
        return BCM2711_PM_WATCHDOG.lock().is_some();
    }

    #[cfg(not(feature = "kernel"))]
    {
        false
    }
}

#[cfg(any(feature = "kernel", test))]
fn rsts_fastboot_reboot_value(current: u32) -> u32 {
    PM_PASSWORD | ((current & !PM_RSTS_COHESIX_FASTBOOT_MASK) | PM_RSTS_COHESIX_FASTBOOT_MAGIC)
}

#[cfg(any(feature = "kernel", test))]
const fn rsts_has_fastboot_marker(value: u32) -> bool {
    value & PM_RSTS_COHESIX_FASTBOOT_MASK == PM_RSTS_COHESIX_FASTBOOT_MAGIC
}

/// Request a platform reboot.
pub fn request_reboot() -> Result<(), RebootError> {
    #[cfg(test)]
    if TEST_BACKEND_AVAILABLE.load(Ordering::SeqCst) {
        TEST_REBOOT_REQUESTS.fetch_add(1, Ordering::SeqCst);
        return Ok(());
    }

    #[cfg(feature = "kernel")]
    {
        let watchdog = BCM2711_PM_WATCHDOG.lock();
        let Some(watchdog) = watchdog.as_ref() else {
            return Err(RebootError::BackendUnavailable);
        };
        let rstc = watchdog
            .read_u32(PM_RSTC_OFFSET)
            .map_err(|_| RebootError::RegisterAccess)?
            & !PM_RSTC_WRCFG_MASK;
        let rsts = watchdog
            .read_u32(PM_RSTS_OFFSET)
            .map_err(|_| RebootError::RegisterAccess)?;
        watchdog
            .write_u32(PM_RSTS_OFFSET, rsts_fastboot_reboot_value(rsts))
            .map_err(|_| RebootError::RegisterAccess)?;
        let marker = watchdog
            .read_u32(PM_RSTS_OFFSET)
            .map_err(|_| RebootError::RegisterAccess)?;
        if !rsts_has_fastboot_marker(marker) {
            watchdog
                .write_u32(PM_RSTS_OFFSET, rsts_fastboot_reboot_value(rsts))
                .map_err(|_| RebootError::RegisterAccess)?;
            let marker = watchdog
                .read_u32(PM_RSTS_OFFSET)
                .map_err(|_| RebootError::RegisterAccess)?;
            if !rsts_has_fastboot_marker(marker) {
                return Err(RebootError::RegisterAccess);
            }
        }
        watchdog
            .write_u32(PM_WDOG_OFFSET, PM_PASSWORD | PM_WDOG_RESET_TICKS)
            .map_err(|_| RebootError::RegisterAccess)?;
        watchdog
            .write_u32(
                PM_RSTC_OFFSET,
                PM_PASSWORD | rstc | PM_RSTC_WRCFG_FULL_RESET,
            )
            .map_err(|_| RebootError::RegisterAccess)?;
        loop {
            core::hint::spin_loop();
        }
    }

    #[cfg(not(feature = "kernel"))]
    {
        Err(RebootError::BackendUnavailable)
    }
}

/// Reset the host-side reboot test hook.
#[cfg(test)]
pub fn reset_test_backend() {
    TEST_BACKEND_AVAILABLE.store(false, Ordering::SeqCst);
    TEST_REBOOT_REQUESTS.store(0, Ordering::SeqCst);
}

/// Enable or disable the host-side reboot test hook.
#[cfg(test)]
pub fn set_test_backend_available(available: bool) {
    TEST_BACKEND_AVAILABLE.store(available, Ordering::SeqCst);
}

/// Return the number of host-side reboot requests observed by tests.
#[cfg(test)]
#[must_use]
pub fn test_reboot_requests() -> usize {
    TEST_REBOOT_REQUESTS.load(Ordering::SeqCst)
}

/// Serialize tests that mutate the host-side reboot backend hook.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_REBOOT_LOCK
        .lock()
        .expect("reboot test lock should not be poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_test_backend_tracks_reboot_requests() {
        let _guard = test_lock();
        reset_test_backend();
        assert!(!backend_available());
        assert_eq!(request_reboot(), Err(RebootError::BackendUnavailable));
        set_test_backend_available(true);
        assert!(backend_available());
        assert_eq!(request_reboot(), Ok(()));
        assert_eq!(test_reboot_requests(), 1);
        reset_test_backend();
    }

    #[test]
    fn fastboot_reboot_marker_preserves_rsts_payload_bits() {
        assert_eq!(
            PM_RSTS_COHESIX_FASTBOOT_MASK & PM_RSTS_PI_FIRMWARE_PARTITION_MASK,
            0
        );

        let current = PM_RSTS_PI_FIRMWARE_PARTITION_MASK | 0x0000_a020 | 0x00aa_0000;
        let encoded = rsts_fastboot_reboot_value(current);
        let low_payload_mask = 0x00ff_ffff;
        let preserved_payload_mask = low_payload_mask & !PM_RSTS_COHESIX_FASTBOOT_MASK;

        assert_eq!(
            encoded & PM_RSTS_COHESIX_FASTBOOT_MASK,
            PM_RSTS_COHESIX_FASTBOOT_MAGIC
        );
        assert_eq!(
            encoded & preserved_payload_mask,
            current & preserved_payload_mask
        );
        assert_eq!(encoded & PM_PASSWORD, PM_PASSWORD);
    }

    #[test]
    fn fastboot_marker_detection_ignores_non_marker_bits() {
        assert!(rsts_has_fastboot_marker(
            PM_RSTS_COHESIX_FASTBOOT_MAGIC | PM_RSTS_PI_FIRMWARE_PARTITION_MASK | 0x0000_a020
        ));
        assert!(!rsts_has_fastboot_marker(
            0x00aa_0000 | PM_RSTS_PI_FIRMWARE_PARTITION_MASK
        ));
    }
}
