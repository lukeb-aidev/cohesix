// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the arch/aarch64/timer module for root-task.
// Author: Lukas Bower

//! Timer helpers tailored to the selected seL4 AArch64 target.

#![allow(unsafe_code)]

#[cfg(feature = "timers-arch-counter")]
use core::arch::asm;

const FALLBACK_QEMU_VIRT_TIMER_HZ: u64 = 62_500_000;

/// Return the architected timer frequency configured by the seL4 kernel for
/// the selected platform.
///
/// The value is emitted by `build.rs` from the active `SEL4_BUILD_DIR`
/// generated `platform_gen.h`. Host-side tests that do not select a seL4 build
/// keep the historical QEMU fallback, but hardware images must get this value
/// from the generated seL4 artifacts.
#[must_use]
pub fn timer_freq_hz() -> u64 {
    option_env!("SEL4_TIMER_CLOCK_HZ")
        .and_then(parse_u64)
        .unwrap_or(FALLBACK_QEMU_VIRT_TIMER_HZ)
}

/// Return the selected EL0 counter kind for diagnostics.
#[must_use]
pub fn timer_counter_kind() -> &'static str {
    option_env!("SEL4_TIMER_COUNTER_KIND").unwrap_or("virtual")
}

/// Convert a timer period in milliseconds into architectural counter cycles.
#[must_use]
pub fn timer_period_cycles(freq_hz: u64, period_ms: u64) -> u64 {
    if freq_hz == 0 {
        return 1;
    }

    let clamped_period = period_ms.max(1);
    let cycles = ((freq_hz as u128) * (clamped_period as u128) / 1_000u128) as u64;
    cycles.max(1)
}

/// Read the EL0 virtual counter used by the Pi profile for telemetry and
/// cooperative timer polling.
#[must_use]
pub fn timer_counter_ticks() -> u64 {
    #[cfg(feature = "timers-arch-counter")]
    {
        read_cntvct()
    }
    #[cfg(not(feature = "timers-arch-counter"))]
    {
        0
    }
}

fn parse_u64(value: &str) -> Option<u64> {
    value.parse().ok()
}

#[cfg(feature = "timers-arch-counter")]
#[inline]
fn read_cntvct() -> u64 {
    let value: u64;
    // SAFETY: `timers-arch-counter` is accepted only when the selected seL4
    // build exports CNTVCT_EL0/CNTFRQ_EL0 to EL0. CNTVCT is read-only and is
    // used here only for bounded timer/latency telemetry.
    unsafe {
        asm!("mrs {value}, cntvct_el0", value = out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::timer_period_cycles;

    #[test]
    fn pi4_five_ms_period_uses_sel4_uboot_clock() {
        assert_eq!(timer_period_cycles(54_000_000, 5), 270_000);
    }

    #[test]
    fn qemu_five_ms_period_uses_virt_clock() {
        assert_eq!(timer_period_cycles(62_500_000, 5), 312_500);
    }
}
