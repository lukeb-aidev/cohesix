// Copyright © 2026 Lukas Bower
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
        crate::bootstrap::log::with_raw_uart_lock(|| {
            crate::sel4::debug_put_bytes_unlocked(s.as_bytes())
        });
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
        crate::bootstrap::log::with_raw_uart_lock(|| {
            crate::sel4::debug_put_line_unlocked(line.as_bytes());
            if line.starts_with("audit ") {
                crate::sel4::debug_put_bytes_unlocked(b"cohesix> ");
            }
        });
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = line;
    }
}
