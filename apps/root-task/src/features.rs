// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compile-time feature manifest and guard rails for root-task builds.
// Author: Lukas Bower

//! Compile-time feature manifest and guard rails for root-task builds.

#[cfg(feature = "kernel")]
use crate::bootstrap::log as boot_log;

/// Public dev umbrella flag for bring-up and instrumentation.
pub const COHESIX_DEV: bool = cfg!(feature = "cohesix-dev");

/// Human-readable summary for the dev umbrella.
pub const COHESIX_DEV_SUMMARY: &str =
    "cohesix-dev enabled (dev-virt cache-trace control-trace net-diag timer-trace)";

#[cfg(all(feature = "bypass-timers", feature = "bypass-timers-ipc"))]
compile_error!("features `bypass-timers` and `bypass-timers-ipc` are mutually exclusive");

#[cfg(all(feature = "timers-arch-counter", feature = "bypass-timers"))]
compile_error!("feature `timers-arch-counter` cannot be combined with `bypass-timers`");

#[cfg(all(feature = "timers-arch-counter", feature = "bypass-timers-ipc"))]
compile_error!("feature `timers-arch-counter` cannot be combined with `bypass-timers-ipc`");

#[cfg(all(feature = "virtio-mmio-legacy", not(feature = "net-backend-virtio")))]
compile_error!("feature `virtio-mmio-legacy` requires `net-backend-virtio`");

#[cfg(all(feature = "net-virtio-tx-v2", not(feature = "net-backend-virtio")))]
compile_error!("feature `net-virtio-tx-v2` requires `net-backend-virtio`");

#[cfg(all(feature = "virtio_guard_queue", not(feature = "net-backend-virtio")))]
compile_error!("feature `virtio_guard_queue` requires `net-backend-virtio`");

/// Emit a single boot audit line when the dev umbrella is enabled.
#[cfg(feature = "cohesix-dev")]
pub fn emit_dev_umbrella_audit() {
    #[cfg(feature = "kernel")]
    {
        boot_log::force_uart_line(COHESIX_DEV_SUMMARY);
    }
}

/// No-op when the dev umbrella is disabled.
#[cfg(not(feature = "cohesix-dev"))]
pub fn emit_dev_umbrella_audit() {}
