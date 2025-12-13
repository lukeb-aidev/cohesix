// Author: Lukas Bower
//! Raw UART debug helpers that bypass the standard logging pipeline.

/// Write a string directly to the seL4 debug console without relying on the
/// PL011 MMIO mapping.
///
/// This helper is best-effort and intentionally ignores errors to avoid
/// disturbing control-flow when instrumentation is needed during bootstrap.
pub fn debug_uart_str(s: &str) {
    #[cfg(feature = "kernel")]
    {
        for byte in s.bytes() {
            crate::sel4::debug_put_char(i32::from(byte));
        }
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = s;
    }
}

/// Emit a short raw debug marker without relying on the UART MMIO base being
/// installed.
///
/// This is intended for ultra-early diagnostics when the logger might be
/// wedged or the runtime is mid-transition.
pub fn debug_uart_raw_marker() {
    debug_uart_str("RAW-ALIVE\n");
}
