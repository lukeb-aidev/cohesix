// Author: Lukas Bower

//! Minimal userland entrypoints exposed by the root task.

use core::fmt::Write;

use crate::platform::Platform;

/// Start the userland console or Cohesix shell over the serial transport.
#[allow(clippy::module_name_repetitions)]
pub fn start_console_or_cohsh<P: Platform>(platform: &P) -> ! {
    serial_console::run(platform)
}

/// Serial console fallback presented during early bring-up.
pub mod serial_console {
    use super::*;

    struct PlatformWriter<'a, P: Platform> {
        platform: &'a P,
    }

    impl<'a, P: Platform> core::fmt::Write for PlatformWriter<'a, P> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for byte in s.as_bytes() {
                self.platform.putc(*byte);
            }
            Ok(())
        }
    }

    /// Run a minimal interactive loop that echoes input and keeps the prompt alive.
    pub fn run<P: Platform>(platform: &P) -> ! {
        let mut writer = PlatformWriter { platform };
        let _ = writeln!(writer);
        let _ = writeln!(writer, "[Cohesix] Root console ready. Type 'help'.");
        let _ = write!(writer, "> ");

        loop {
            if let Some(byte) = platform.getc_nonblock() {
                platform.putc(byte);
                if byte == b'\r' || byte == b'\n' {
                    let _ = write!(writer, "\r\n> ");
                }
            } else {
                core::hint::spin_loop();
            }
        }
    }
}
