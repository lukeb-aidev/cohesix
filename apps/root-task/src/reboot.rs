// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide authenticated reboot backend wiring for the root-task console.
// Author: Lukas Bower

//! Authenticated platform reboot support for root-task console commands.

#[cfg(feature = "kernel")]
use core::sync::atomic::{fence, Ordering as FenceOrdering};
#[cfg(test)]
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(feature = "kernel")]
use crate::hal::{DeviceHal, HalError, MappedRegisterPages, MappedRegisterWindow};
#[cfg(feature = "kernel")]
use spin::Mutex;

#[cfg(feature = "kernel")]
const BCM2711_PM_BASE: usize = 0xfe10_0000;
#[cfg(any(feature = "kernel", test))]
const PM_RSTC_OFFSET: usize = 0x1c;
#[cfg(any(feature = "kernel", test))]
const PM_RSTS_OFFSET: usize = 0x20;
#[cfg(any(feature = "kernel", test))]
const PM_WDOG_OFFSET: usize = 0x24;
#[cfg(any(feature = "kernel", test))]
const PM_PASSWORD: u32 = 0x5a00_0000;
#[cfg(any(feature = "kernel", test))]
const PM_RSTC_WRCFG_MASK: u32 = 0x0000_0030;
#[cfg(any(feature = "kernel", test))]
const PM_RSTC_WRCFG_FULL_RESET: u32 = 0x0000_0020;
#[cfg(any(feature = "kernel", test))]
const PM_WDOG_RESET_TICKS: u32 = 10;
#[cfg(any(feature = "kernel", test))]
const PM_RSTS_PI_FIRMWARE_PARTITION_MASK: u32 = 0x0000_0555;
#[cfg(any(feature = "kernel", test))]
const PM_RSTS_SOFTWARE_RESET_STATUS_MASK: u32 = 0x0000_0400;
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
fn rsts_fastboot_reboot_fallback_value() -> u32 {
    PM_PASSWORD | PM_RSTS_COHESIX_FASTBOOT_MAGIC
}

#[cfg(any(feature = "kernel", test))]
fn rstc_full_reset_value(current: u32) -> u32 {
    PM_PASSWORD | (current & !PM_RSTC_WRCFG_MASK) | PM_RSTC_WRCFG_FULL_RESET
}

#[cfg(any(feature = "kernel", test))]
const fn rsts_has_fastboot_marker(value: u32) -> bool {
    value & PM_RSTS_COHESIX_FASTBOOT_MASK == PM_RSTS_COHESIX_FASTBOOT_MAGIC
}

#[cfg(feature = "kernel")]
#[inline(always)]
fn pm_mmio_write_barrier() {
    fence(FenceOrdering::SeqCst);
}

#[cfg(feature = "kernel")]
fn emit_bcm2711_fastboot_marker_trace(
    before: Result<u32, RebootError>,
    programmed: u32,
    readback: Result<u32, RebootError>,
) {
    use core::fmt::Write as _;

    let mut line = heapless::String::<192>::new();
    let before_value = before.unwrap_or(0);
    let readback_value = readback.unwrap_or(0);
    let before_status = if before.is_ok() { "ok" } else { "err" };
    let readback_status = if readback.is_ok() { "ok" } else { "err" };
    let _ = write!(
        line,
        "[reboot] fastboot-marker before_status={} before=0x{:08x} programmed=0x{:08x} readback_status={} readback=0x{:08x} high=0x{:08x} reset=0x{:08x}",
        before_status,
        before_value,
        programmed & !PM_PASSWORD,
        readback_status,
        readback_value,
        readback_value & PM_RSTS_COHESIX_FASTBOOT_MASK,
        readback_value & PM_RSTS_SOFTWARE_RESET_STATUS_MASK,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(any(feature = "kernel", test))]
trait Bcm2711PmAccess {
    fn read_u32(&mut self, offset: usize) -> Result<u32, RebootError>;
    fn write_u32(&mut self, offset: usize, value: u32) -> Result<(), RebootError>;
    fn write_barrier(&mut self);
}

#[cfg(feature = "kernel")]
struct KernelBcm2711PmAccess<'a> {
    watchdog: &'a MappedRegisterWindow,
}

#[cfg(feature = "kernel")]
impl Bcm2711PmAccess for KernelBcm2711PmAccess<'_> {
    fn read_u32(&mut self, offset: usize) -> Result<u32, RebootError> {
        self.watchdog
            .read_u32(offset)
            .map_err(|_| RebootError::RegisterAccess)
    }

    fn write_u32(&mut self, offset: usize, value: u32) -> Result<(), RebootError> {
        self.watchdog
            .write_u32(offset, value)
            .map_err(|_| RebootError::RegisterAccess)
    }

    fn write_barrier(&mut self) {
        pm_mmio_write_barrier();
    }
}

#[cfg(any(feature = "kernel", test))]
fn write_u32_release<A>(access: &mut A, offset: usize, value: u32) -> Result<(), RebootError>
where
    A: Bcm2711PmAccess,
{
    access.write_u32(offset, value)?;
    access.write_barrier();
    Ok(())
}

#[cfg(any(feature = "kernel", test))]
fn prepare_bcm2711_watchdog_reset<A>(access: &mut A) -> Result<(), RebootError>
where
    A: Bcm2711PmAccess,
{
    let rsts_before = access.read_u32(PM_RSTS_OFFSET);
    let rsts_value = match rsts_before {
        Ok(rsts) => rsts_fastboot_reboot_value(rsts),
        Err(_) => rsts_fastboot_reboot_fallback_value(),
    };
    write_u32_release(access, PM_RSTS_OFFSET, rsts_value)?;
    let rsts_readback = access.read_u32(PM_RSTS_OFFSET);
    #[cfg(feature = "kernel")]
    emit_bcm2711_fastboot_marker_trace(rsts_before, rsts_value, rsts_readback);
    access.write_barrier();
    write_u32_release(access, PM_WDOG_OFFSET, PM_PASSWORD | PM_WDOG_RESET_TICKS)?;
    let rstc = access.read_u32(PM_RSTC_OFFSET)?;
    write_u32_release(access, PM_RSTC_OFFSET, rstc_full_reset_value(rstc))?;
    Ok(())
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
        let mut access = KernelBcm2711PmAccess { watchdog };
        prepare_bcm2711_watchdog_reset(&mut access)?;
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
    use std::collections::VecDeque;
    use std::vec::Vec;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeAccessEvent {
        Read(usize),
        Write(usize, u32),
        Barrier,
    }

    struct FakePmAccess {
        reads: VecDeque<Result<u32, RebootError>>,
        events: Vec<FakeAccessEvent>,
    }

    impl FakePmAccess {
        fn new(reads: impl IntoIterator<Item = Result<u32, RebootError>>) -> Self {
            Self {
                reads: reads.into_iter().collect(),
                events: Vec::new(),
            }
        }
    }

    impl Bcm2711PmAccess for FakePmAccess {
        fn read_u32(&mut self, offset: usize) -> Result<u32, RebootError> {
            self.events.push(FakeAccessEvent::Read(offset));
            self.reads
                .pop_front()
                .unwrap_or(Err(RebootError::RegisterAccess))
        }

        fn write_u32(&mut self, offset: usize, value: u32) -> Result<(), RebootError> {
            self.events.push(FakeAccessEvent::Write(offset, value));
            Ok(())
        }

        fn write_barrier(&mut self) {
            self.events.push(FakeAccessEvent::Barrier);
        }
    }

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
        let marker_mask = PM_RSTS_COHESIX_FASTBOOT_MASK;
        let preserved_payload_mask = low_payload_mask & !marker_mask;

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
    fn watchdog_reset_sequence_drains_fastboot_marker_before_arming_reset() {
        let rsts = PM_RSTS_PI_FIRMWARE_PARTITION_MASK | 0x0000_a000 | 0x00aa_0000;
        let rstc = 0x0000_0010;
        let mut access = FakePmAccess::new([Ok(rsts), Ok(rsts), Ok(rstc)]);

        assert_eq!(prepare_bcm2711_watchdog_reset(&mut access), Ok(()));

        assert_eq!(
            access.events,
            vec![
                FakeAccessEvent::Read(PM_RSTS_OFFSET),
                FakeAccessEvent::Write(PM_RSTS_OFFSET, rsts_fastboot_reboot_value(rsts)),
                FakeAccessEvent::Barrier,
                FakeAccessEvent::Read(PM_RSTS_OFFSET),
                FakeAccessEvent::Barrier,
                FakeAccessEvent::Write(PM_WDOG_OFFSET, PM_PASSWORD | PM_WDOG_RESET_TICKS),
                FakeAccessEvent::Barrier,
                FakeAccessEvent::Read(PM_RSTC_OFFSET),
                FakeAccessEvent::Write(PM_RSTC_OFFSET, rstc_full_reset_value(rstc)),
                FakeAccessEvent::Barrier,
            ]
        );
    }

    #[test]
    fn watchdog_reset_sequence_writes_fallback_marker_when_rsts_read_fails() {
        let rstc = 0x0000_0030;
        let mut access = FakePmAccess::new([Err(RebootError::RegisterAccess), Ok(0), Ok(rstc)]);

        assert_eq!(prepare_bcm2711_watchdog_reset(&mut access), Ok(()));

        assert_eq!(
            access.events,
            vec![
                FakeAccessEvent::Read(PM_RSTS_OFFSET),
                FakeAccessEvent::Write(PM_RSTS_OFFSET, rsts_fastboot_reboot_fallback_value()),
                FakeAccessEvent::Barrier,
                FakeAccessEvent::Read(PM_RSTS_OFFSET),
                FakeAccessEvent::Barrier,
                FakeAccessEvent::Write(PM_WDOG_OFFSET, PM_PASSWORD | PM_WDOG_RESET_TICKS),
                FakeAccessEvent::Barrier,
                FakeAccessEvent::Read(PM_RSTC_OFFSET),
                FakeAccessEvent::Write(PM_RSTC_OFFSET, rstc_full_reset_value(rstc)),
                FakeAccessEvent::Barrier,
            ]
        );
    }

    #[test]
    fn fastboot_marker_detection_requires_cohesix_high_marker() {
        assert!(rsts_has_fastboot_marker(
            PM_RSTS_COHESIX_FASTBOOT_MAGIC | PM_RSTS_PI_FIRMWARE_PARTITION_MASK | 0x0000_a020
        ));
        assert!(!rsts_has_fastboot_marker(
            PM_RSTS_SOFTWARE_RESET_STATUS_MASK | PM_RSTS_PI_FIRMWARE_PARTITION_MASK | 0x0000_0020
        ));
        assert!(!rsts_has_fastboot_marker(
            0x00aa_0000 | PM_RSTS_PI_FIRMWARE_PARTITION_MASK | 0x0000_0020
        ));
    }

    #[test]
    fn rstc_full_reset_value_preserves_non_wrcfg_bits() {
        let current = 0x0000_a030;
        let encoded = rstc_full_reset_value(current);

        assert_eq!(encoded & PM_PASSWORD, PM_PASSWORD);
        assert_eq!(encoded & PM_RSTC_WRCFG_MASK, PM_RSTC_WRCFG_FULL_RESET);
        assert_eq!(
            encoded & !PM_PASSWORD & !PM_RSTC_WRCFG_MASK,
            current & !PM_RSTC_WRCFG_MASK
        );
    }
}
