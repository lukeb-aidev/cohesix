// Author: Lukas Bower
#![no_std]

use core::fmt::{self, Write};
use core::panic::PanicInfo;

use sel4_sys::seL4_DebugPutChar;

struct DebugWriter;

impl Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            unsafe {
                seL4_DebugPutChar(byte);
            }
        }
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut writer = DebugWriter;
    let _ = writeln!(writer, "[sel4-panicking] panic: {info}");
    loop {
        unsafe {
            seL4_DebugPutChar(b'!');
        }
    }
}
