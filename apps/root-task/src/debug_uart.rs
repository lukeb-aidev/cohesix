// Author: Lukas Bower
//! Raw UART debug helpers that bypass the standard logging pipeline.

/// Write a string directly to the seL4 debug console without relying on MMIO mappings.
///
/// This helper is best-effort and intentionally ignores errors to avoid
/// disturbing control-flow when instrumentation is needed during bootstrap.
pub fn debug_uart_str(s: &str) {
    #[cfg(feature = "kernel")]
    {
        if crate::log_buffer::log_channel_active() {
            crate::log_buffer::append_log_bytes(s.as_bytes());
            return;
        }
        for byte in s.bytes() {
            crate::sel4::debug_put_char(i32::from(byte));
        }
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = s;
    }
}
