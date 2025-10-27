// Author: Lukas Bower

#![cfg(feature = "kernel")]

//! Bootstrap handler primitives built around function pointers.

use core::fmt;

use sel4_sys::seL4_Word;

/// Result returned by a bootstrap handler invocation.
pub type HandlerResult = Result<(), HandlerError>;

/// Canonical handler pointer type for bootstrap IPC verbs.
pub type Handler = fn(&[seL4_Word]) -> HandlerResult;

/// Structured error surfaced by bootstrap handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerError {
    /// The payload slice did not meet the handler's expectations.
    InvalidPayload,
    /// Handler executed but reported a logical failure.
    Failure,
}

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPayload => write!(f, "invalid payload"),
            Self::Failure => write!(f, "handler reported failure"),
        }
    }
}

/// Collection of handler entry points for the bootstrap dispatcher.
#[derive(Clone, Copy)]
pub struct HandlerTable {
    /// Handler invoked for `BootstrapOp::Attach` messages.
    pub attach: Handler,
    /// Handler invoked for `BootstrapOp::Spawn` messages.
    pub spawn: Handler,
    /// Handler invoked for `BootstrapOp::Log` messages.
    pub log: Handler,
}

impl HandlerTable {
    /// Constructs a handler table from the supplied function pointers.
    #[must_use]
    pub const fn new(attach: Handler, spawn: Handler, log: Handler) -> Self {
        Self { attach, spawn, log }
    }
}

#[cfg(debug_assertions)]
fn assert_text_fn(handler: Handler) {
    extern "C" {
        static __text_start: u8;
        static __text_end: u8;
    }

    let ptr = handler as usize;
    let lo = core::ptr::addr_of!(__text_start) as usize;
    let hi = core::ptr::addr_of!(__text_end) as usize;
    assert!(
        ptr >= lo && ptr < hi,
        "handler ptr not in .text: 0x{ptr:016x}"
    );
}

/// Invokes the supplied handler after verifying its provenance.
#[inline(always)]
pub fn call_handler(handler: Handler, words: &[seL4_Word]) -> HandlerResult {
    #[cfg(debug_assertions)]
    {
        assert_text_fn(handler);
    }
    handler(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_layout_matches_usize() {
        assert_eq!(
            core::mem::size_of::<Handler>(),
            core::mem::size_of::<usize>()
        );
        assert_eq!(
            core::mem::align_of::<Handler>(),
            core::mem::align_of::<usize>()
        );
    }
}
