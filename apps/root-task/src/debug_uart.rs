// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the debug_uart module for root-task.
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

/// Emit a single line to the debug UART, bypassing the log buffer.
pub fn debug_uart_line(line: &str) {
    #[cfg(feature = "kernel")]
    {
        for byte in line.bytes() {
            crate::sel4::debug_put_char_raw(byte);
        }
        crate::sel4::debug_put_char_raw(b'\r');
        crate::sel4::debug_put_char_raw(b'\n');
        if line.starts_with("audit ") {
            for byte in b"cohesix> " {
                crate::sel4::debug_put_char_raw(*byte);
            }
        }
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = line;
    }
}
