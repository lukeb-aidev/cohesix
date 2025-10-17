// Author: Lukas Bower

//! Platform abstraction used to bridge minimal early-console access between host and kernel builds.

use core::ffi::c_void;

#[cfg(feature = "kernel")]
use crate::sel4::{debug_poll_char, debug_put_char};

/// Minimal facilities required by the root task before higher-level services are initialised.
pub trait Platform {
    /// Emits a single byte to the debug console.
    fn putc(&self, byte: u8);

    /// Attempts to retrieve a byte from the debug console without blocking.
    fn getc_nonblock(&self) -> Option<u8>;

    /// Returns a raw pointer to boot information when available.
    fn bootinfo_ptr(&self) -> *const c_void;
}

#[cfg(feature = "kernel")]
/// Platform implementation backed by seL4 kernel primitives.
pub struct SeL4Platform {
    bootinfo: *const c_void,
}

#[cfg(feature = "kernel")]
impl SeL4Platform {
    /// Creates a platform wrapper using the boot information pointer provided by seL4.
    pub const fn new(ptr: *const c_void) -> Self {
        Self { bootinfo: ptr }
    }
}

#[cfg(feature = "kernel")]
impl Platform for SeL4Platform {
    fn putc(&self, byte: u8) {
        debug_put_char(byte as i32);
    }

    fn getc_nonblock(&self) -> Option<u8> {
        let ch = debug_poll_char();
        if ch >= 0 {
            Some(ch as u8)
        } else {
            None
        }
    }

    fn bootinfo_ptr(&self) -> *const c_void {
        self.bootinfo
    }
}

#[cfg(not(feature = "kernel"))]
/// Host-mode platform that proxies early console I/O through `print!`.
pub struct HostPlatform;

#[cfg(not(feature = "kernel"))]
impl Platform for HostPlatform {
    fn putc(&self, byte: u8) {
        print!("{}", byte as char);
    }

    fn getc_nonblock(&self) -> Option<u8> {
        None
    }

    fn bootinfo_ptr(&self) -> *const c_void {
        core::ptr::null()
    }
}
