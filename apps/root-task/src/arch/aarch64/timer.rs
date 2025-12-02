// Author: Lukas Bower

//! Timer helpers tailored to the seL4 AArch64 QEMU virt target.

/// Return the architected timer frequency configured by the seL4 kernel for
/// the QEMU virt platform.
///
/// The kernel build bundled with this repository programs CNTFRQ to
/// 62.5 MHz; accessing CNTFRQ from EL0 is not permitted on seL4 so we expose
/// the value directly to userland initialisation.
#[must_use]
pub fn timer_freq_hz() -> u64 {
    const SEL4_QEMU_VIRT_TIMER_HZ: u64 = 62_500_000;
    SEL4_QEMU_VIRT_TIMER_HZ
}
