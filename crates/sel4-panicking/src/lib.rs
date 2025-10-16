// Author: Lukas Bower
#![no_std]

use core::fmt::{self, Write};
use core::panic::PanicInfo;

struct DebugWriter;

impl Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            unsafe {
                sel4::debug_put_char(byte as i32);
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
            sel4::debug_put_char(b'!' as i32);
        }
    }
}
