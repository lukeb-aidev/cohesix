// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the arch/aarch64/timer module for root-task.
// Author: Lukas Bower

//! Timer helpers tailored to the selected seL4 AArch64 target.

#![allow(unsafe_code)]

#[cfg(all(feature = "timers-arch-counter", target_os = "none"))]
use core::arch::asm;

/// Return the architected timer frequency configured by the seL4 kernel for
/// the selected platform.
///
/// The value is emitted by `build.rs` from the active `SEL4_BUILD_DIR`
/// generated `platform_gen.h`. Runtime-eligible QEMU and Pi feature closures
/// select `timers-arch-counter`, so missing, malformed, or zero generated truth
/// is fatal instead of degrading to a platform constant.
#[must_use]
pub fn timer_freq_hz() -> u64 {
    #[cfg(all(feature = "timers-arch-counter", target_os = "none"))]
    {
        let frequency = option_env!("SEL4_TIMER_CLOCK_HZ")
            .and_then(parse_u64)
            .expect("architected-counter builds require generated SEL4_TIMER_CLOCK_HZ");
        assert!(
            frequency != 0,
            "architected-counter builds require nonzero SEL4_TIMER_CLOCK_HZ"
        );
        frequency
    }

    #[cfg(not(all(feature = "timers-arch-counter", target_os = "none")))]
    {
        0
    }
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
    #[cfg(all(feature = "timers-arch-counter", target_os = "none"))]
    {
        read_cntvct()
    }
    #[cfg(not(all(feature = "timers-arch-counter", target_os = "none")))]
    {
        0
    }
}

fn parse_u64(value: &str) -> Option<u64> {
    value.parse().ok()
}

#[cfg(all(feature = "timers-arch-counter", target_os = "none"))]
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
    use super::{parse_u64, timer_period_cycles};

    #[test]
    fn generated_frequency_parser_distinguishes_zero_and_invalid_truth() {
        assert_eq!(parse_u64("24000000"), Some(24_000_000));
        assert_eq!(parse_u64("0"), Some(0));
        assert_eq!(parse_u64("not-a-clock"), None);
    }

    #[test]
    fn pi4_five_ms_period_uses_sel4_uboot_clock() {
        assert_eq!(timer_period_cycles(54_000_000, 5), 270_000);
    }

    #[test]
    fn qemu_five_ms_period_uses_virt_clock() {
        assert_eq!(timer_period_cycles(24_000_000, 5), 120_000);
    }
}
