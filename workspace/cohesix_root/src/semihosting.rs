// CLASSIFICATION: COMMUNITY
// Filename: semihosting.rs v0.1
// Author: Lukas Bower
// Date Modified: 2030-03-09

#![allow(dead_code)]

#[cfg(feature = "semihosting")]
mod imp {
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

    const MODE_DISABLED: u8 = 0;
    const MODE_STDOUT: u8 = 1;
    const MODE_STDERR: u8 = 2;
    const MODE_FILE: u8 = 3;

    const HANDLE_STDOUT: usize = 1;
    const HANDLE_STDERR: usize = 2;

    const SYS_OPEN: usize = 0x01;
    const SYS_WRITE: usize = 0x05;

    static MODE: AtomicU8 = AtomicU8::new(MODE_DISABLED);
    static FILE_HANDLE: AtomicUsize = AtomicUsize::new(usize::MAX);

    #[repr(C)]
    struct OpenArgs {
        path: *const u8,
        mode: usize,
        len: usize,
    }

    #[repr(C)]
    struct WriteArgs {
        handle: usize,
        buffer: *const u8,
        len: usize,
    }

    fn semihost_call(op: usize, arg: usize) -> usize {
        let mut operation = op;
        let mut argument = arg;
        unsafe {
            core::arch::asm!(
                "hlt #0xf000",
                inout("x0") operation,
                inout("x1") argument,
                options(nostack, preserves_flags)
            );
        }
        operation
    }

    fn open_file(path: &str) -> Option<usize> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut bytes = Vec::from(trimmed.as_bytes());
        if !bytes.ends_with(&[0]) {
            bytes.push(0);
        }
        let args = OpenArgs {
            path: bytes.as_ptr(),
            mode: 4, // write, truncate
            len: trimmed.len(),
        };
        let result = semihost_call(SYS_OPEN, &args as *const _ as usize);
        if result == usize::MAX {
            None
        } else {
            Some(result)
        }
    }

    fn write_handle(handle: usize, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let args = WriteArgs {
            handle,
            buffer: bytes.as_ptr(),
            len: bytes.len(),
        };
        let _ = semihost_call(SYS_WRITE, &args as *const _ as usize);
    }

    fn configure_stdout(mode: u8) {
        MODE.store(mode, Ordering::Release);
    }

    fn configure_file(path: &str) {
        if let Some(handle) = open_file(path) {
            FILE_HANDLE.store(handle, Ordering::Release);
            MODE.store(MODE_FILE, Ordering::Release);
        }
    }

    pub fn handle_bootarg(token: &str) {
        let (key, value) = match token.split_once('=') {
            Some((k, v)) => (k, v),
            None => (token, ""),
        };
        if !key.eq_ignore_ascii_case("coh.semihost") {
            return;
        }
        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("stdout") {
            configure_stdout(MODE_STDOUT);
        } else if value.eq_ignore_ascii_case("stderr") {
            configure_stdout(MODE_STDERR);
        } else if value.eq_ignore_ascii_case("off") {
            MODE.store(MODE_DISABLED, Ordering::Release);
        } else if let Some(path) = value.strip_prefix("file:") {
            configure_file(path);
        }
    }

    pub fn write_byte(byte: u8) {
        match MODE.load(Ordering::Acquire) {
            MODE_DISABLED => {}
            MODE_STDOUT => write_handle(HANDLE_STDOUT, core::slice::from_ref(&byte)),
            MODE_STDERR => write_handle(HANDLE_STDERR, core::slice::from_ref(&byte)),
            MODE_FILE => {
                let handle = FILE_HANDLE.load(Ordering::Acquire);
                if handle != usize::MAX {
                    write_handle(handle, core::slice::from_ref(&byte));
                }
            }
            _ => {}
        }
    }

    #[cfg(test)]
    pub fn test_reset() {
        MODE.store(MODE_DISABLED, Ordering::Release);
        FILE_HANDLE.store(usize::MAX, Ordering::Release);
    }

    #[cfg(test)]
    pub fn test_mode() -> u8 {
        MODE.load(Ordering::Acquire)
    }

    #[cfg(all(test, feature = "semihosting"))]
    mod tests {
        use super::*;

        #[test]
        fn enables_stdout_mode() {
            test_reset();
            handle_bootarg("coh.semihost=stdout");
            assert_eq!(test_mode(), MODE_STDOUT);
        }
    }
}

#[cfg(not(feature = "semihosting"))]
mod imp {
    pub fn handle_bootarg(_token: &str) {}
    pub fn write_byte(_byte: u8) {}
    #[cfg(test)]
    pub fn test_reset() {}
    #[cfg(test)]
    pub fn test_mode() -> u8 {
        0
    }
}

pub use imp::*;
