// Author: Lukas Bower
//! Boot-time diagnostic flags exposed to bootstrap helpers.

use core::sync::atomic::{AtomicBool, Ordering};

/// Controls whether bootstrap code should emit detailed destination traces before seL4 syscalls.
static TRACE_DEST: AtomicBool = AtomicBool::new(false);

/// Returns `true` when destination tracing is enabled.
#[inline(always)]
pub fn trace_dest() -> bool {
    TRACE_DEST.load(Ordering::Relaxed)
}

/// Enables or disables destination tracing at runtime.
#[inline(always)]
pub fn set_trace_dest(enabled: bool) {
    TRACE_DEST.store(enabled, Ordering::Relaxed);
}
