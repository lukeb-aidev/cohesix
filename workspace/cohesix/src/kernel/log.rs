// CLASSIFICATION: COMMUNITY
// Filename: log.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-02-02

use core::fmt;

#[cfg(feature = "kernel_uart")]
use core::fmt::Write;

#[cfg(feature = "kernel_uart")]
extern "C" {
    fn uart_write(ptr: *const u8, len: usize);
}

#[cfg(feature = "kernel_uart")]
struct UartWriter;

#[cfg(feature = "kernel_uart")]
impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            uart_write(s.as_ptr(), s.len());
        }
        Ok(())
    }
}

#[cfg(feature = "kernel_uart")]
pub fn uart_write_fmt(args: fmt::Arguments) {
    let _ = UartWriter.write_fmt(args);
}

#[cfg(not(feature = "kernel_uart"))]
#[allow(unused_variables)]
pub fn uart_write_fmt(_args: fmt::Arguments) {}

#[macro_export]
macro_rules! coherr {
    ($($arg:tt)*) => {{
        $crate::kernel::log::uart_write_fmt(core::format_args!($($arg)*));
    }};
}
