// Author: Lukas Bower
//! Raw UART debug helpers that bypass the standard logging pipeline.

/// Write a string directly to the UART using the lowest-level PL011 primitive.
///
/// This helper is best-effort and intentionally ignores errors to avoid
/// disturbing control-flow when instrumentation is needed during bootstrap.
pub fn debug_uart_str(s: &str) {
    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    {
        for byte in s.bytes() {
            crate::uart::pl011::write_byte(byte);
        }
    }

    #[cfg(not(all(feature = "serial-console", feature = "kernel")))]
    {
        let _ = s;
    }
}

/// Emit a short raw UART marker without relying on the logging subsystem.
///
/// This is intended for ultra-early diagnostics when the logger might be
/// wedged or the runtime is mid-transition.
pub fn debug_uart_raw_marker() {
    debug_uart_str("RAW-ALIVE\n");
}
